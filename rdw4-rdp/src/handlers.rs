use std::sync::{mpsc, Arc};

use freerdp::{
    channels::{
        cliprdr::{Format, GeneralCapabilities},
        encomsp::ParticipantCreated,
    },
    client::{
        CliprdrClientContext, CliprdrFormat, CliprdrHandler, Context, EncomspClientContext,
        EncomspHandler,
    },
    graphics::Pointer,
    locale::keyboard_init_ex,
    update, RdpError, Result, PIXEL_FORMAT_BGRA32,
};
use futures::{executor::block_on, SinkExt};

use crate::util::mime_from_format;

#[derive(Debug)]
pub(crate) struct CursorInner {
    pub(crate) data: Vec<u8>,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[derive(Debug, Clone)]
pub(crate) struct Cursor {
    pub(crate) inner: Arc<CursorInner>,
}

#[derive(Debug)]
pub(crate) enum RdpEvent {
    Authenticate {
        settings: freerdp::Settings,
        tx: mpsc::Sender<Result<freerdp::Settings>>,
    },
    Connected,
    Disconnected,
    DesktopResize {
        w: u32,
        h: u32,
    },
    Update {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    },
    CursorSet(Cursor),
    CursorSetNull,
    CursorSetDefault,
    ClipboardSetContent {
        formats: Vec<&'static str>,
    },
    ClipboardData {
        data: Vec<u8>,
    },
    ClipboardDataRequest {
        format: Format,
    },
}

#[derive(Debug)]
struct RdpPointerHandler {
    // FIXME: really need to replace new/free with Box...
    cursor: Option<Cursor>,
}

impl freerdp::graphics::PointerHandler for RdpPointerHandler {
    type ContextHandler = RdpContextHandler;

    fn new(
        &mut self,
        context: &mut Context<Self::ContextHandler>,
        pointer: &Pointer,
    ) -> Result<()> {
        let gdi = context.gdi().ok_or(RdpError::Unsupported)?;
        let data = pointer.bgra_data(gdi.palette())?;
        let width = pointer.width() as _;
        let height = pointer.height() as _;
        let x = pointer.x() as _;
        let y = pointer.y() as _;
        self.cursor = Some(Cursor {
            inner: Arc::new(CursorInner {
                data,
                width,
                height,
                x,
                y,
            }),
        });
        Ok(())
    }

    fn free(&mut self, _context: &mut Context<Self::ContextHandler>, _pointer: &Pointer) {
        self.cursor.take();
    }

    fn set(
        &mut self,
        context: &mut Context<Self::ContextHandler>,
        _pointer: &Pointer,
    ) -> Result<()> {
        let cursor = self.cursor.as_ref().ok_or(RdpError::Unsupported)?;
        context.handler.send(RdpEvent::CursorSet(cursor.clone()))
    }

    fn set_null(context: &mut Context<Self::ContextHandler>) -> Result<()> {
        context.handler.send(RdpEvent::CursorSetNull)
    }

    fn set_default(context: &mut Context<Self::ContextHandler>) -> Result<()> {
        context.handler.send(RdpEvent::CursorSetDefault)
    }
}

#[derive(Debug)]
struct RdpUpdateHandler;

impl freerdp::update::UpdateHandler for RdpUpdateHandler {
    type ContextHandler = RdpContextHandler;

    fn begin_paint(context: &mut Context<Self::ContextHandler>) -> Result<()> {
        let gdi = context.gdi().ok_or(RdpError::Unsupported)?;
        let mut primary = gdi.primary().ok_or(RdpError::Unsupported)?;
        primary.hdc().hwnd().invalid().set_null(true);
        Ok(())
    }

    fn end_paint(context: &mut Context<Self::ContextHandler>) -> Result<()> {
        let gdi = context.gdi().ok_or(RdpError::Unsupported)?;
        let mut primary = gdi.primary().ok_or(RdpError::Unsupported)?;
        let invalid = primary.hdc().hwnd().invalid();
        if invalid.null() {
            return Ok(());
        }
        let (x, y, w, h) = (invalid.x(), invalid.y(), invalid.w(), invalid.h());
        let x = u32::try_from(x)?;
        let y = u32::try_from(y)?;
        let w = u32::try_from(w)?;
        let h = u32::try_from(h)?;

        context.handler.send_update_buffer(x, y, w, h)
    }

    fn set_bounds(
        _context: &mut Context<Self::ContextHandler>,
        bounds: &update::Bounds,
    ) -> Result<()> {
        dbg!(bounds);
        Ok(())
    }

    fn synchronize(_context: &mut Context<Self::ContextHandler>) -> Result<()> {
        dbg!();
        Ok(())
    }

    fn desktop_resize(context: &mut Context<Self::ContextHandler>) -> Result<()> {
        let mut gdi = context.gdi().ok_or(RdpError::Unsupported)?;
        let (w, h) = (
            context.settings.desktop_width(),
            context.settings.desktop_height(),
        );
        gdi.resize(w, h)?;
        context.handler.send_desktop_resize(w, h)
    }
}

#[derive(Debug)]
pub(crate) struct RdpEncomspHandler {
    context: RdpContextHandler,
}

impl RdpEncomspHandler {
    fn new(context: RdpContextHandler) -> Self {
        Self { context }
    }
}

impl EncomspHandler for RdpEncomspHandler {
    fn participant_created(
        &mut self,
        _ctxt: &mut EncomspClientContext,
        participant: &ParticipantCreated,
    ) -> Result<()> {
        dbg!(&participant);
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct RdpClipHandler {
    context: RdpContextHandler,
}

impl RdpClipHandler {
    fn new(context: RdpContextHandler) -> Self {
        Self { context }
    }
}

impl CliprdrHandler for RdpClipHandler {
    fn monitor_ready(&mut self, context: &mut CliprdrClientContext) -> Result<()> {
        let capabilities = GeneralCapabilities::USE_LONG_FORMAT_NAMES;
        // | GeneralCapabilities::STREAM_FILECLIP_ENABLED
        // | GeneralCapabilities::FILECLIP_NO_FILE_PATHS
        // | GeneralCapabilities::HUGE_FILE_SUPPORT_ENABLED;
        context.send_client_general_capabilities(&capabilities)?;
        context.send_client_format_list(&[])
    }

    fn server_capabilities(
        &mut self,
        _context: &mut CliprdrClientContext,
        capabilities: Option<&freerdp::channels::cliprdr::GeneralCapabilities>,
    ) -> Result<()> {
        dbg!(capabilities);
        Ok(())
    }

    fn server_format_list(
        &mut self,
        context: &mut CliprdrClientContext,
        formats: &[CliprdrFormat],
    ) -> Result<()> {
        let formats: Vec<_> = formats
            .iter()
            .filter_map(|f| f.id.and_then(mime_from_format))
            .collect();
        self.context.send_clipboard_set_content(formats)?;
        context.send_client_format_list_response(true)
    }

    fn server_format_list_response(&mut self, _context: &mut CliprdrClientContext) -> Result<()> {
        Ok(())
    }

    fn server_format_data_request(
        &mut self,
        _context: &mut CliprdrClientContext,
        format: Format,
    ) -> Result<()> {
        self.context.send_clipboard_data_request(format)
    }

    fn server_format_data_response(
        &mut self,
        _context: &mut CliprdrClientContext,
        data: &[u8],
    ) -> Result<()> {
        self.context.send_clipboard_data(data.to_vec())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RdpContextHandler {
    tx: futures::channel::mpsc::UnboundedSender<RdpEvent>,
}

impl RdpContextHandler {
    pub(crate) fn new(tx: futures::channel::mpsc::UnboundedSender<RdpEvent>) -> Self {
        Self { tx }
    }

    pub(crate) fn close(&mut self) {
        self.tx.close_channel();
    }

    fn send(&mut self, event: RdpEvent) -> Result<()> {
        let mut tx = self.tx.clone();
        block_on(async move { tx.send(event).await })
            .map_err(|e| RdpError::Failed(format!("{}", e)))?;
        Ok(())
    }

    fn send_update_buffer(&mut self, x: u32, y: u32, w: u32, h: u32) -> Result<()> {
        self.send(RdpEvent::Update { x, y, w, h })
    }

    fn send_desktop_resize(&mut self, w: u32, h: u32) -> Result<()> {
        self.send(RdpEvent::DesktopResize { w, h })
    }

    fn send_clipboard_set_content(&mut self, formats: Vec<&'static str>) -> Result<()> {
        self.send(RdpEvent::ClipboardSetContent { formats })
    }

    fn send_clipboard_data(&mut self, data: Vec<u8>) -> Result<()> {
        self.send(RdpEvent::ClipboardData { data })
    }

    fn send_clipboard_data_request(&mut self, format: Format) -> Result<()> {
        self.send(RdpEvent::ClipboardDataRequest { format })
    }
}

impl freerdp::client::Handler for RdpContextHandler {
    fn authenticate(&mut self, context: &mut Context<Self>) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        self.send(RdpEvent::Authenticate {
            tx,
            settings: context.settings.clone(),
        })?;
        let settings = rx.recv().unwrap()?;
        context.settings.clone_from(&settings);
        Ok(())
    }

    fn post_connect(&mut self, context: &mut Context<Self>) -> Result<()> {
        context.instance.gdi_init(PIXEL_FORMAT_BGRA32)?;

        let gdi = context.gdi().ok_or(RdpError::Unsupported)?;
        let mut graphics = context.graphics().ok_or(RdpError::Unsupported)?;
        let mut update = context.update().ok_or(RdpError::Unsupported)?;

        let (Some(w), Some(h)) = (gdi.width(), gdi.height()) else {
            return Err(RdpError::Failed("No GDI dimensions".into()));
        };

        graphics.register_pointer::<RdpPointerHandler>();
        update.register::<RdpUpdateHandler>();

        let _ = keyboard_init_ex(
            context.settings.keyboard_layout(),
            context.settings.keyboard_remapping_list().as_deref(),
        );

        context.handler.send_desktop_resize(w, h)?;
        self.send(RdpEvent::Connected)?;
        Ok(())
    }

    fn post_disconnect(&mut self, context: &mut Context<Self>)
    where
        Self: Sized,
    {
        context.instance.gdi_uninit();
        let _ = self.send(RdpEvent::Disconnected);
    }

    fn clipboard_connected(&mut self, clip: &mut CliprdrClientContext) {
        clip.register_handler(RdpClipHandler::new(self.clone()));
    }

    fn encomsp_connected(&mut self, encomsp: &mut EncomspClientContext) {
        encomsp.register_handler(RdpEncomspHandler::new(self.clone()));
    }
}

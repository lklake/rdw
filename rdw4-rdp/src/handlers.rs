use std::sync::{mpsc, Arc, Mutex};

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
        let ctxt = context.handler_mut().ok_or(RdpError::Unsupported)?;
        let cursor = self.cursor.as_ref().ok_or(RdpError::Unsupported)?;
        ctxt.send(RdpEvent::CursorSet(cursor.clone()))
    }

    fn set_null(context: &mut Context<Self::ContextHandler>) -> Result<()> {
        let ctxt = context.handler_mut().ok_or(RdpError::Unsupported)?;
        ctxt.send(RdpEvent::CursorSetNull)
    }

    fn set_default(context: &mut Context<Self::ContextHandler>) -> Result<()> {
        let ctxt = context.handler_mut().ok_or(RdpError::Unsupported)?;
        ctxt.send(RdpEvent::CursorSetDefault)
    }
}

#[derive(Debug)]
struct RdpUpdateHandler;

impl freerdp::update::UpdateHandler for RdpUpdateHandler {
    type ContextHandler = RdpContextHandler;

    fn begin_paint(context: &mut freerdp::client::Context<Self::ContextHandler>) -> Result<()> {
        let gdi = context.gdi().ok_or(RdpError::Unsupported)?;
        let mut primary = gdi.primary().ok_or(RdpError::Unsupported)?;
        primary.hdc().hwnd().invalid().set_null(true);
        Ok(())
    }

    fn end_paint(context: &mut freerdp::client::Context<Self::ContextHandler>) -> Result<()> {
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

        let handler = context.handler_mut().unwrap();
        handler.send_update_buffer(x, y, w, h)
    }

    fn set_bounds(
        _context: &mut freerdp::client::Context<Self::ContextHandler>,
        bounds: &update::Bounds,
    ) -> Result<()> {
        dbg!(bounds);
        Ok(())
    }

    fn synchronize(_context: &mut freerdp::client::Context<Self::ContextHandler>) -> Result<()> {
        dbg!();
        Ok(())
    }

    fn desktop_resize(context: &mut freerdp::client::Context<Self::ContextHandler>) -> Result<()> {
        let mut gdi = context.gdi().ok_or(RdpError::Unsupported)?;
        let (w, h) = (
            context.settings.desktop_width(),
            context.settings.desktop_height(),
        );
        gdi.resize(w, h)?;
        let handler = context.handler_mut().unwrap();
        handler.send_desktop_resize(w, h)
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
            .filter_map(|f| f.id.map(mime_from_format).flatten())
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

#[derive(Debug)]
struct Inner {
    tx: futures::channel::mpsc::Sender<RdpEvent>,
    clip: Option<CliprdrClientContext>,
    encomsp: Option<EncomspClientContext>,
}

#[derive(Clone, Debug)]
pub(crate) struct RdpContextHandler {
    inner: Arc<Mutex<Inner>>,
}

impl RdpContextHandler {
    pub(crate) fn new(tx: futures::channel::mpsc::Sender<RdpEvent>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                tx,
                clip: None,
                encomsp: None,
            })),
        }
    }

    fn send(&mut self, event: RdpEvent) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        block_on(async { inner.tx.feed(event).await })
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

    pub(crate) fn client_clipboard_request(&self, format: Format) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let clip = inner
            .clip
            .as_mut()
            .ok_or(RdpError::Failed("No clipboard context!".into()))?;
        clip.send_client_format_data_request(format)
    }

    pub(crate) fn client_clipboard_format_list(&self, list: &[CliprdrFormat]) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let clip = inner
            .clip
            .as_mut()
            .ok_or(RdpError::Failed("No clipboard context!".into()))?;
        clip.send_client_format_list(list)
    }

    pub(crate) fn client_clipboard_data(&self, data: Option<Vec<u8>>) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let clip = inner
            .clip
            .as_mut()
            .ok_or(RdpError::Failed("No clipboard context!".into()))?;
        clip.send_client_format_data_response(data.as_deref())
    }
}

impl freerdp::client::Handler for RdpContextHandler {
    fn authenticate(&mut self, context: &mut freerdp::client::Context<Self>) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        self.send(RdpEvent::Authenticate {
            tx,
            settings: context.settings.clone(),
        })?;
        let settings = rx.recv().unwrap()?;
        context.settings.clone_from(&settings);
        Ok(())
    }

    fn post_connect(&mut self, context: &mut freerdp::client::Context<Self>) -> Result<()> {
        context.instance.gdi_init(PIXEL_FORMAT_BGRA32)?;

        let gdi = context.gdi().ok_or(RdpError::Unsupported)?;
        let mut graphics = context.graphics().ok_or(RdpError::Unsupported)?;
        let mut update = context.update().ok_or(RdpError::Unsupported)?;

        let (w, h) = match (gdi.width(), gdi.height()) {
            (Some(w), Some(h)) => (w, h),
            _ => return Err(RdpError::Failed("No GDI dimensions".into())),
        };

        graphics.register_pointer::<RdpPointerHandler>();
        update.register::<RdpUpdateHandler>();

        let _ = keyboard_init_ex(
            context.settings.keyboard_layout(),
            context.settings.keyboard_remapping_list().as_deref(),
        );

        let handler = context.handler_mut().unwrap();
        handler.send_desktop_resize(w, h)
    }

    fn clipboard_connected(&mut self, mut clip: CliprdrClientContext) {
        clip.register_handler(RdpClipHandler::new(self.clone()));
        let mut inner = self.inner.lock().unwrap();
        inner.clip = Some(clip);
    }

    fn encomsp_connected(&mut self, mut encomsp: EncomspClientContext) {
        encomsp.register_handler(RdpEncomspHandler::new(self.clone()));
        let mut inner = self.inner.lock().unwrap();
        inner.encomsp = Some(encomsp);
    }
}

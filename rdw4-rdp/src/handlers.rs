use std::sync::{mpsc, Arc};

use freerdp::{
    client::Context, graphics::Pointer, locale::keyboard_init_ex, update, RdpError, Result,
    PIXEL_FORMAT_BGRA32,
};
use futures::{executor::block_on, SinkExt};

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
pub(crate) struct RdpContextHandler {
    tx: futures::channel::mpsc::Sender<RdpEvent>,
}

impl RdpContextHandler {
    pub(crate) fn new(tx: futures::channel::mpsc::Sender<RdpEvent>) -> Self {
        Self { tx }
    }

    fn send(&mut self, event: RdpEvent) -> Result<()> {
        block_on(async { self.tx.send(event).await })
            .map_err(|e| RdpError::Failed(format!("{}", e)))?;
        Ok(())
    }

    fn send_update_buffer(&mut self, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        let x = u32::try_from(x)?;
        let y = u32::try_from(y)?;
        let w = u32::try_from(w)?;
        let h = u32::try_from(h)?;
        self.send(RdpEvent::Update { x, y, w, h })
    }

    fn send_desktop_resize(&mut self, w: u32, h: u32) -> Result<()> {
        self.send(RdpEvent::DesktopResize { w, h })
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
}

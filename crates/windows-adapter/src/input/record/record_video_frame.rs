// Part of Agent Record Adapter (see NOTICE).
//! One video frame: the recorded area copied from the screen with the
//! pointer drawn on it, as top-down 32-bit BGRX pixels.

use agent_desktop_core::{AdapterError, ErrorCode};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
    CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HDC, ReleaseDC,
    SRCCOPY, SelectObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo, ICONINFO,
};

use super::record_video_area::Area;

pub(super) struct Grabber {
    area: Area,
    pixels: Vec<u8>,
}

impl Grabber {
    pub(super) fn new(area: Area) -> Self {
        let size = area.width as usize * area.height as usize * 4;
        Self {
            area,
            pixels: vec![0; size],
        }
    }

    /// The area as it is on the screen now.
    pub(super) fn grab(&mut self) -> Result<&[u8], AdapterError> {
        let Area {
            x,
            y,
            width,
            height,
        } = self.area;
        let (w, h) = (width as i32, height as i32);
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            if screen.is_null() {
                return Err(frame_error("no screen device context"));
            }
            let memory = CreateCompatibleDC(screen);
            let bitmap = CreateCompatibleBitmap(screen, w, h);
            let previous = SelectObject(memory, bitmap);
            let copied = BitBlt(memory, 0, 0, w, h, screen, x, y, SRCCOPY | CAPTUREBLT) != 0;
            if copied {
                draw_pointer(memory, self.area);
            }
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };
            SelectObject(memory, previous);
            let lines = GetDIBits(
                memory,
                bitmap,
                0,
                height,
                self.pixels.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            );
            DeleteObject(bitmap);
            DeleteDC(memory);
            ReleaseDC(std::ptr::null_mut(), screen);
            if !copied || lines == 0 {
                return Err(frame_error("could not copy the screen"));
            }
        }
        Ok(&self.pixels)
    }
}

unsafe fn draw_pointer(target: HDC, area: Area) {
    let mut cursor: CURSORINFO = unsafe { std::mem::zeroed() };
    cursor.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
    if unsafe { GetCursorInfo(&mut cursor) } == 0 || cursor.flags & CURSOR_SHOWING == 0 {
        return;
    }
    let mut icon: ICONINFO = unsafe { std::mem::zeroed() };
    if unsafe { GetIconInfo(cursor.hCursor, &mut icon) } == 0 {
        return;
    }
    let x = cursor.ptScreenPos.x - icon.xHotspot as i32 - area.x;
    let y = cursor.ptScreenPos.y - icon.yHotspot as i32 - area.y;
    unsafe {
        DrawIconEx(
            target,
            x,
            y,
            cursor.hCursor,
            0,
            0,
            0,
            std::ptr::null_mut(),
            DI_NORMAL,
        );
        if !icon.hbmMask.is_null() {
            DeleteObject(icon.hbmMask);
        }
        if !icon.hbmColor.is_null() {
            DeleteObject(icon.hbmColor);
        }
    }
}

fn frame_error(message: &str) -> AdapterError {
    AdapterError::new(ErrorCode::ActionFailed, format!("Video: {message}"))
}

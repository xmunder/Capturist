#[cfg(target_os = "windows")]
mod win {
    use std::sync::{Mutex, OnceLock};

    use std::ptr;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, FrameRect, InvalidateRect,
        HBRUSH, PAINTSTRUCT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
        GetMessageW, GetSystemMetrics, LoadCursorW, RegisterClassW, SetCursor, SetForegroundWindow,
        SetLayeredWindowAttributes, ShowWindow, TranslateMessage, HMENU, IDC_CROSS, LWA_ALPHA,
        LWA_COLORKEY, MSG, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN, SW_SHOW, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
        WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_POPUP,
    };

    use crate::capture::models::Region;

    const MIN_SELECTION_EDGE_PX: i32 = 5;
    const OVERLAY_DIM_ALPHA: u8 = 120;
    const OVERLAY_COLOR: COLORREF = COLORREF(0x00000000);
    const SELECTION_HOLE_COLOR: COLORREF = COLORREF(0x00030201);
    const SELECTION_BORDER_THICKNESS_PX: i32 = 2;

    #[derive(Default, Copy, Clone)]
    struct State {
        selecting: bool,
        start: POINT,
        current: POINT,
        rect: RECT,
        cancelled: bool,
        done: bool,
    }

    static STATE: OnceLock<Mutex<State>> = OnceLock::new();

    fn state() -> &'static Mutex<State> {
        STATE.get_or_init(|| Mutex::new(State::default()))
    }

    fn update_rect(s: &mut State) {
        let left = s.start.x.min(s.current.x);
        let top = s.start.y.min(s.current.y);
        let right = s.start.x.max(s.current.x);
        let bottom = s.start.y.max(s.current.y);
        s.rect = RECT {
            left,
            top,
            right,
            bottom,
        };
    }

    fn has_area(rect: &RECT) -> bool {
        rect.right > rect.left && rect.bottom > rect.top
    }

    fn same_rect(a: &RECT, b: &RECT) -> bool {
        a.left == b.left && a.top == b.top && a.right == b.right && a.bottom == b.bottom
    }

    fn rect_intersection(a: &RECT, b: &RECT) -> Option<RECT> {
        let left = a.left.max(b.left);
        let top = a.top.max(b.top);
        let right = a.right.min(b.right);
        let bottom = a.bottom.min(b.bottom);
        let intersection = RECT {
            left,
            top,
            right,
            bottom,
        };
        if has_area(&intersection) {
            Some(intersection)
        } else {
            None
        }
    }

    fn expand_rect(rect: RECT, padding: i32) -> RECT {
        RECT {
            left: rect.left - padding,
            top: rect.top - padding,
            right: rect.right + padding,
            bottom: rect.bottom + padding,
        }
    }

    unsafe fn request_repaint(hwnd: HWND) {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }

    unsafe fn request_repaint_rect(hwnd: HWND, rect: &RECT) {
        let _ = InvalidateRect(Some(hwnd), Some(rect), false);
    }

    unsafe fn paint_overlay(hwnd: HWND) {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        if hdc.is_invalid() {
            let _ = EndPaint(hwnd, &ps);
            return;
        }

        let mut client_rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut client_rect);
        let paint_rect = if has_area(&ps.rcPaint) {
            ps.rcPaint
        } else {
            client_rect
        };

        let base_brush = CreateSolidBrush(OVERLAY_COLOR);
        if !base_brush.0.is_null() {
            let _ = FillRect(hdc, &paint_rect, base_brush);
            let _ = DeleteObject(base_brush.into());
        }

        let selection = {
            let s = state().lock().expect("estado overlay poisoned");
            s.rect
        };

        if has_area(&selection) {
            // La región seleccionada usa un color-key transparente para imitar Snipping Tool:
            // fuera de la selección queda oscurecido y dentro se ve el contenido real.
            let hole_brush = CreateSolidBrush(SELECTION_HOLE_COLOR);
            if !hole_brush.0.is_null() {
                if let Some(hole_region) = rect_intersection(&selection, &paint_rect) {
                    let _ = FillRect(hdc, &hole_region, hole_brush);
                }
                let _ = DeleteObject(hole_brush.into());
            }

            let border_brush = CreateSolidBrush(COLORREF(0x00FFFFFF));
            if !border_brush.0.is_null() {
                let border_bounds = expand_rect(selection, SELECTION_BORDER_THICKNESS_PX);
                if rect_intersection(&border_bounds, &paint_rect).is_some() {
                    let mut inner = selection;
                    let _ = FrameRect(hdc, &selection, border_brush);
                    if inner.right - inner.left > 2 && inner.bottom - inner.top > 2 {
                        inner.left += 1;
                        inner.top += 1;
                        inner.right -= 1;
                        inner.bottom -= 1;
                        let _ = FrameRect(hdc, &inner, border_brush);
                    }
                }
                let _ = DeleteObject(border_brush.into());
            }
        }

        let _ = EndPaint(hwnd, &ps);
    }

    unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        match msg {
            WM_LBUTTONDOWN => {
                let mut s = state().lock().expect("estado overlay poisoned");
                s.selecting = true;
                s.start.x = (l.0 & 0xFFFF) as i16 as i32;
                s.start.y = ((l.0 >> 16) & 0xFFFF) as i16 as i32;
                s.current = s.start;
                update_rect(&mut s);
                windows_sys::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd.0);
                request_repaint(hwnd);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let mut dirty_old = None;
                let mut dirty_new = None;
                {
                    let mut s = state().lock().expect("estado overlay poisoned");
                    if s.selecting {
                        s.current.x = (l.0 & 0xFFFF) as i16 as i32;
                        s.current.y = ((l.0 >> 16) & 0xFFFF) as i16 as i32;
                        let old_rect = s.rect;
                        update_rect(&mut s);
                        if same_rect(&old_rect, &s.rect) {
                            return LRESULT(0);
                        }
                        let dirty_padding = SELECTION_BORDER_THICKNESS_PX + 1;
                        dirty_old = Some(expand_rect(old_rect, dirty_padding));
                        dirty_new = Some(expand_rect(s.rect, dirty_padding));
                    }
                }
                if let Some(old_rect) = dirty_old {
                    request_repaint_rect(hwnd, &old_rect);
                }
                if let Some(new_rect) = dirty_new {
                    request_repaint_rect(hwnd, &new_rect);
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let mut s = state().lock().expect("estado overlay poisoned");
                if s.selecting {
                    s.selecting = false;
                    s.done = true;
                    update_rect(&mut s);
                    windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();

                    let width = (s.rect.right - s.rect.left).abs();
                    let height = (s.rect.bottom - s.rect.top).abs();
                    if width < MIN_SELECTION_EDGE_PX || height < MIN_SELECTION_EDGE_PX {
                        s.done = false;
                        s.rect = RECT::default();
                        request_repaint(hwnd);
                        return LRESULT(0);
                    }
                }
                LRESULT(0)
            }
            WM_RBUTTONDOWN | WM_KEYDOWN => {
                if msg == WM_KEYDOWN && w.0 as u32 != 0x1B {
                    return DefWindowProcW(hwnd, msg, w, l);
                }
                let mut s = state().lock().expect("estado overlay poisoned");
                s.cancelled = true;
                s.done = true;
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                paint_overlay(hwnd);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, w, l),
        }
    }

    pub fn select_region() -> Result<Option<Region>, String> {
        unsafe {
            {
                let mut s = state().lock().expect("estado overlay poisoned");
                *s = State::default();
            }

            let class_name: Vec<u16> = "RegionOverlay".encode_utf16().chain([0]).collect();
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
                hbrBackground: HBRUSH::default(),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };

            RegisterClassW(&wc);

            let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(class_name.as_ptr()),
                WS_POPUP,
                x,
                y,
                w,
                h,
                Some(HWND(ptr::null_mut())),
                Some(HMENU(ptr::null_mut())),
                None,
                None,
            )
            .map_err(|e| e.to_string())?;

            if hwnd.0.is_null() {
                return Err("No se pudo crear la ventana overlay".to_string());
            }

            SetCursor(Some(LoadCursorW(None, IDC_CROSS).unwrap_or_default()));
            let _ = SetLayeredWindowAttributes(
                hwnd,
                SELECTION_HOLE_COLOR,
                OVERLAY_DIM_ALPHA,
                LWA_ALPHA | LWA_COLORKEY,
            );
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            request_repaint(hwnd);

            let mut msg = MSG::default();
            loop {
                let res = GetMessageW(&mut msg, Some(HWND(ptr::null_mut())), 0, 0);
                if res.0 == 0 || res.0 == -1 {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);

                let done = {
                    let s = state().lock().expect("estado overlay poisoned");
                    s.done
                };
                if done {
                    break;
                }
            }

            let _ = DestroyWindow(hwnd);

            let s = state().lock().expect("estado overlay poisoned");
            if s.cancelled {
                return Ok(None);
            }

            let rect = s.rect;
            let width = (rect.right - rect.left).max(1) as u32;
            let height = (rect.bottom - rect.top).max(1) as u32;
            let region = Region {
                x: (x + rect.left).max(0) as u32,
                y: (y + rect.top).max(0) as u32,
                width,
                height,
            };

            Ok(Some(region))
        }
    }
}

#[cfg(target_os = "windows")]
pub fn select_region() -> Result<Option<crate::capture::models::Region>, String> {
    win::select_region()
}

#[cfg(not(target_os = "windows"))]
pub fn select_region() -> Result<Option<crate::capture::models::Region>, String> {
    Err("Overlay solo disponible en Windows".to_string())
}

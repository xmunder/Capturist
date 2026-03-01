mod overlay_win;

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Debug, Clone, Copy)]
pub struct SelectionBounds {
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: u32,
    pub height: u32,
}

pub use overlay_win::{select_region, select_region_with_bounds};

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::select_region;

    #[test]
    fn select_region_fuera_de_windows_devuelve_error_de_plataforma() {
        let err = select_region().expect_err("fuera de windows debe devolver error controlado");
        assert!(err.contains("Windows"));
    }
}

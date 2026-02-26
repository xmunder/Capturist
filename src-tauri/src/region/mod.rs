mod overlay_win;

pub use overlay_win::select_region;

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::select_region;

    #[test]
    fn select_region_fuera_de_windows_devuelve_error_de_plataforma() {
        let err = select_region().expect_err("fuera de windows debe devolver error controlado");
        assert!(err.contains("Windows"));
    }
}

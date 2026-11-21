// ==============================================================================
// OWNERSHIP:
//   Keeps a converted file's icon where it was on the Desktop. The swap makes
//   a new file under the name, and Explorer puts new files in the first free
//   slot; this reads the icon's spot before and puts the new file back there.
//   Best effort only: it never affects the conversion itself.
// ==============================================================================
use std::path::{Path, PathBuf};

/// Where the icon of `path` sits on the Desktop, if it is shown there.
pub struct Spot {
    x: i32,
    y: i32,
}

#[cfg(windows)]
mod shell {
    use super::Spot;
    use std::path::Path;
    use windows::core::{Interface, PWSTR};
    use windows::Win32::Foundation::POINT;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IServiceProvider,
        CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::Variant::{VARIANT, VT_I4};
    use windows::Win32::UI::Shell::Common::{ITEMIDLIST, STRRET};
    use windows::Win32::UI::Shell::{
        IEnumIDList, IFolderView, ILFree, IShellBrowser, IShellFolder, IShellWindows,
        SID_STopLevelBrowser, ShellWindows, StrRetToStrW, CSIDL_DESKTOP, SHGDN_FORPARSING,
        SVGIO_ALLVIEW, SVSI_POSITIONITEM, SWC_DESKTOP, SWFO_NEEDDISPATCH,
    };

    /// COM for this thread, undone when dropped.
    struct Com(bool);
    impl Com {
        fn init() -> Self {
            Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
        }
    }
    impl Drop for Com {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    /// The Desktop's own folder view in Explorer.
    ///
    /// # Safety
    /// COM must be initialised on this thread ([`Com::init`]) and stay so
    /// while the returned view is used. The view belongs to this thread's
    /// apartment: do not send it to another thread.
    unsafe fn desktop_view() -> windows::core::Result<IFolderView> {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
        let mut location = VARIANT::default();
        (*location.Anonymous.Anonymous).vt = VT_I4;
        (*location.Anonymous.Anonymous).Anonymous.lVal = CSIDL_DESKTOP as i32;
        let empty = VARIANT::default();
        let mut hwnd = 0;
        let found =
            windows.FindWindowSW(&location, &empty, SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH)?;
        let provider: IServiceProvider = found.cast()?;
        let browser: IShellBrowser = provider.QueryService(&SID_STopLevelBrowser)?;
        browser.QueryActiveShellView()?.cast()
    }

    /// Calls `f` with the view's item for `path` (compared by full path).
    ///
    /// # Safety
    /// As for [`desktop_view`]. The item id passed to `f` is valid only during
    /// the call: it is freed (`ILFree`) right after, so `f` must not keep it.
    unsafe fn with_item<T>(
        view: &IFolderView,
        path: &Path,
        f: impl FnOnce(*const ITEMIDLIST) -> Option<T>,
    ) -> Option<T> {
        let folder: IShellFolder = view.GetFolder().ok()?;
        let items: IEnumIDList = view.Items(SVGIO_ALLVIEW).ok()?;
        let wanted = path.to_string_lossy().to_lowercase();
        loop {
            let mut pidl = [std::ptr::null_mut::<ITEMIDLIST>()];
            let mut fetched = 0u32;
            if items.Next(&mut pidl, Some(&mut fetched)).is_err() || fetched == 0 {
                return None;
            }
            let item = pidl[0];
            let mut name = STRRET::default();
            let mut matched = false;
            if folder
                .GetDisplayNameOf(item, SHGDN_FORPARSING, &mut name)
                .is_ok()
            {
                let mut text = PWSTR::null();
                if StrRetToStrW(&mut name, Some(item), &mut text).is_ok() {
                    matched = text.to_string().is_ok_and(|t| t.to_lowercase() == wanted);
                    CoTaskMemFree(Some(text.0 as _));
                }
            }
            if matched {
                let result = f(item);
                ILFree(Some(item));
                return result;
            }
            ILFree(Some(item));
        }
    }

    pub fn spot_of(path: &Path) -> Option<Spot> {
        let _com = Com::init();
        // SAFETY: COM is initialised for this scope (`_com`), the view stays on
        // this thread, and the item id is used only inside the closure.
        unsafe {
            let view = desktop_view().ok()?;
            with_item(&view, path, |item| {
                view.GetItemPosition(item)
                    .ok()
                    .map(|p| Spot { x: p.x, y: p.y })
            })
        }
    }

    /// True once the item was found and moved.
    pub fn place(path: &Path, spot: &Spot) -> bool {
        let _com = Com::init();
        // SAFETY: as in `spot_of`; `items` points at one item id that lives
        // until the call returns.
        unsafe {
            let Ok(view) = desktop_view() else {
                return false;
            };
            with_item(&view, path, |item| {
                let point = POINT {
                    x: spot.x,
                    y: spot.y,
                };
                let items = [item];
                view.SelectAndPositionItems(
                    1,
                    items.as_ptr(),
                    Some(&point),
                    SVSI_POSITIONITEM.0 as u32,
                )
                .ok()
            })
            .is_some()
        }
    }
}

/// Reads the icon's spot before the swap. `None` when the file is not on the
/// Desktop (or Explorer is not running).
pub fn remember(path: &Path) -> Option<Spot> {
    #[cfg(windows)]
    {
        shell::spot_of(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

/// Puts the new file's icon on the remembered spot. Explorer shows the new
/// file a moment after it appears, so this waits for it, off the caller's
/// thread.
pub fn restore(path: PathBuf, spot: Spot) {
    #[cfg(windows)]
    std::thread::spawn(move || {
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if shell::place(&path, &spot) {
                return;
            }
        }
    });
    #[cfg(not(windows))]
    let _ = (path, spot);
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Needs a signed-in Desktop: reads the first Desktop file's spot and
    /// puts it back on that same spot (nothing moves).
    #[test]
    #[ignore = "talks to Explorer"]
    fn reads_and_restores_a_desktop_icon_in_place() {
        let desktop = PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join("Desktop");
        let file = std::fs::read_dir(&desktop)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.is_file())
            .expect("a file on the Desktop");
        let spot = remember(&file).expect("its icon position");
        println!("{} at {},{}", file.display(), spot.x, spot.y);
        assert!(shell::place(&file, &spot));
        let again = remember(&file).unwrap();
        assert_eq!((again.x, again.y), (spot.x, spot.y));
    }
}

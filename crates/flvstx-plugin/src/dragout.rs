//! Windows OLE drag-out of a `.mid` file, so a layer can be dropped onto an FL Studio channel or
//! piano roll (the same mechanism Scaler / Cthulhu use). Must run on the GUI thread while the mouse
//! button is held; `DoDragDrop` blocks until the user drops or cancels.

#![cfg(windows)]

use std::path::Path;
use windows::core::{implement, Result, BOOL, HRESULT};
use windows::Win32::Foundation::{DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC, E_NOTIMPL, HGLOBAL, POINT, S_OK};
use windows::Win32::System::Com::{IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA, FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT};
use windows::Win32::System::Ole::{DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK};
use windows::Win32::System::SystemServices::{MODIFIERKEYS_FLAGS, MK_LBUTTON};
use windows::Win32::UI::Shell::DROPFILES;

const CF_HDROP: u16 = 15;

/// Builds an HGLOBAL holding a DROPFILES structure with one wide file path.
fn make_hdrop(path: &Path) -> Result<HGLOBAL> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0u16, 0u16]).collect();
    let header = std::mem::size_of::<DROPFILES>();
    let total = header + wide.len() * 2;
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, total)?;
        let p = GlobalLock(h) as *mut u8;
        if p.is_null() {
            return Err(windows::core::Error::from_thread());
        }
        let df = p as *mut DROPFILES;
        (*df).pFiles = header as u32;
        (*df).pt = POINT { x: 0, y: 0 };
        (*df).fNC = BOOL(0);
        (*df).fWide = BOOL(1);
        std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, p.add(header), wide.len() * 2);
        let _ = GlobalUnlock(h);
        Ok(h)
    }
}

use std::os::windows::ffi::OsStrExt;

#[implement(IDataObject)]
struct FileData {
    path: std::path::PathBuf,
}

impl IDataObject_Impl for FileData_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> Result<STGMEDIUM> {
        let f = unsafe { &*pformatetcin };
        if f.cfFormat != CF_HDROP || (f.tymed & TYMED_HGLOBAL.0 as u32) == 0 {
            return Err(DV_E_FORMATETC.into());
        }
        let h = make_hdrop(&self.path)?;
        Ok(STGMEDIUM { tymed: TYMED_HGLOBAL.0 as u32, u: STGMEDIUM_0 { hGlobal: h }, pUnkForRelease: std::mem::ManuallyDrop::new(None) })
    }
    fn GetDataHere(&self, _pformatetc: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        let f = unsafe { &*pformatetc };
        if f.cfFormat == CF_HDROP && (f.tymed & TYMED_HGLOBAL.0 as u32) != 0 { S_OK } else { DV_E_FORMATETC }
    }
    fn GetCanonicalFormatEtc(&self, _pformatectin: *const FORMATETC, pformatetcout: *mut FORMATETC) -> HRESULT {
        unsafe {
            (*pformatetcout).ptd = std::ptr::null_mut();
        }
        E_NOTIMPL
    }
    fn SetData(&self, _pformatetc: *const FORMATETC, _pmedium: *const STGMEDIUM, _frelease: BOOL) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn EnumFormatEtc(&self, _dwdirection: u32) -> Result<IEnumFORMATETC> {
        Err(E_NOTIMPL.into())
    }
    fn DAdvise(&self, _pformatetc: *const FORMATETC, _advf: u32, _padvsink: windows::core::Ref<'_, IAdviseSink>) -> Result<u32> {
        Err(E_NOTIMPL.into())
    }
    fn DUnadvise(&self, _dwconnection: u32) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(E_NOTIMPL.into())
    }
}

#[implement(IDropSource)]
struct Source;

impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            DRAGDROP_S_CANCEL
        } else if (grfkeystate.0 & MK_LBUTTON.0) == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }
    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

use std::sync::Mutex;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, PostMessageW, SetTimer, WM_LBUTTONUP};

static PENDING: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);
static RESULT: Mutex<Option<(std::path::PathBuf, bool)>> = Mutex::new(None);
const DRAG_TIMER: usize = 0x7A91;

/// Schedules the drag to start from a timer callback, i.e. outside the GUI framework's event handler
/// (DoDragDrop runs a modal message loop that would otherwise re-enter it). Call while the mouse
/// button is held. Poll [`take_result`] on later frames.
pub fn start_drag_deferred(hwnd: isize, path: &Path) {
    *PENDING.lock().unwrap() = Some(path.to_path_buf());
    unsafe {
        SetTimer(Some(HWND(hwnd as *mut _)), DRAG_TIMER, 1, Some(drag_timer_proc));
    }
}

unsafe extern "system" fn drag_timer_proc(hwnd: HWND, _msg: u32, id: usize, _time: u32) {
    let _ = KillTimer(Some(hwnd), id);
    let Some(path) = PENDING.lock().unwrap().take() else { return };
    let dropped = drag_file(&path);
    *RESULT.lock().unwrap() = Some((path, dropped));
    // OLE swallowed the button-up; give the GUI one so it leaves its drag state.
    let _ = PostMessageW(Some(hwnd), WM_LBUTTONUP, WPARAM(0), LPARAM(0));
}

/// Result of the last deferred drag, once.
pub fn take_result() -> Option<(std::path::PathBuf, bool)> {
    RESULT.lock().unwrap().take()
}

/// Starts an OLE file drag of `path`. Blocks until the drop finishes. Returns true if dropped.
pub fn drag_file(path: &Path) -> bool {
    unsafe {
        let _ = OleInitialize(None);
        let data: IDataObject = FileData { path: path.to_path_buf() }.into();
        let source: IDropSource = Source.into();
        let mut effect = DROPEFFECT(0);
        let hr = DoDragDrop(&data, &source, DROPEFFECT_COPY | DROPEFFECT_LINK, &mut effect);
        hr == DRAGDROP_S_DROP && effect.0 != 0
    }
}

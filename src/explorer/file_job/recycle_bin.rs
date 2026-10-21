//! The Recycle Bin through the shell's `IFileOperation`, as Explorer
//! does: every entry in one operation, its progress reported to our sink
//! (`UpdateProgress`, in the shell's own units -- a percentage, not
//! bytes) and stoppable from it. `SHFileOperationW` had neither
//! (requested: progress and stopping for the Recycle Bin too). Windows
//! only, and never in tests -- there a delete is for good (`run`).

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows::core::{implement, Ref, Result as WinResult, HRESULT, PCWSTR};
use windows::Win32::Foundation::E_ABORT;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, IBindCtx, CLSCTX_ALL, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    FileOperation, IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl, IShellItem, SHCreateItemFromParsingName, FOFX_RECYCLEONDELETE, FOF_ALLOWUNDO,
    FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING,
};

use super::Progress;
use super::JobEntry;


/// Sends every entry to the Recycle Bin in one operation. What doesn't fit
/// there asks Windows' own question before it's deleted for good
/// (`FOF_WANTNUKEWARNING`). The failures, as `run` reports them.
pub(super) fn send(entries: &[JobEntry], progress: &Arc<Progress>, cancel: &Arc<AtomicBool>) -> Vec<(String, io::Error)> {
    // SAFETY: COM is set up for this thread, which the job owns, and taken
    // down below with everything made here already dropped.
    let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if init.is_err() {
        return vec![(label(entries), io::Error::other(format!("couldn't start the shell ({init:?})")))];
    }
    let result = perform(entries, progress, cancel);
    unsafe { CoUninitialize() };
    match result {
        Ok(()) => Vec::new(),
        Err(_) if cancel.load(Ordering::Relaxed) => Vec::new(),
        Err(err) => vec![(label(entries), io::Error::other(format!("couldn't move it to the Recycle Bin ({err})")))],
    }
}


fn perform(entries: &[JobEntry], progress: &Arc<Progress>, cancel: &Arc<AtomicBool>) -> WinResult<()> {
    // SAFETY: plain COM calls on interfaces this function holds; the path
    // buffers outlive each call that reads them.
    unsafe {
        let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        operation.SetOperationFlags(FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI | FOF_WANTNUKEWARNING | FOFX_RECYCLEONDELETE)?;
        let sink: IFileOperationProgressSink = Sink { progress: Arc::clone(progress), cancel: Arc::clone(cancel) }.into();
        let cookie = operation.Advise(&sink)?;
        for entry in entries {
            let item = shell_item(&entry.path)?;
            operation.DeleteItem(&item, None)?;
        }
        let performed = operation.PerformOperations();
        let _ = operation.Unadvise(cookie);
        performed?;
        if operation.GetAnyOperationsAborted()?.as_bool() {
            return Err(E_ABORT.into());
        }
        Ok(())
    }
}


fn shell_item(path: &Path) -> WinResult<IShellItem> {
    use std::os::windows::ffi::OsStrExt;
    let absolute = std::path::absolute(path).map_err(|err| windows::core::Error::new(E_ABORT, err.to_string()))?;
    let wide: Vec<u16> = absolute.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None::<&IBindCtx>) }
}


fn label(entries: &[JobEntry]) -> String {
    match entries {
        [single] => single.name.clone(),
        many => format!("{} entries", many.len()),
    }
}


/// Gets the operation's progress, and stops it once asked to.
#[implement(IFileOperationProgressSink)]
struct Sink {
    progress: Arc<Progress>,
    cancel: Arc<AtomicBool>,
}


impl Sink_Impl {
    fn go_on(&self) -> WinResult<()> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(E_ABORT.into())
        } else {
            Ok(())
        }
    }
}


impl IFileOperationProgressSink_Impl for Sink_Impl {
    fn StartOperations(&self) -> WinResult<()> {
        Ok(())
    }

    fn FinishOperations(&self, _hrresult: HRESULT) -> WinResult<()> {
        Ok(())
    }

    fn PreRenameItem(&self, _flags: u32, _item: Ref<'_, IShellItem>, _new_name: &PCWSTR) -> WinResult<()> {
        self.go_on()
    }

    fn PostRenameItem(&self, _flags: u32, _item: Ref<'_, IShellItem>, _new_name: &PCWSTR, _hr: HRESULT, _created: Ref<'_, IShellItem>) -> WinResult<()> {
        Ok(())
    }

    fn PreMoveItem(&self, _flags: u32, _item: Ref<'_, IShellItem>, _folder: Ref<'_, IShellItem>, _new_name: &PCWSTR) -> WinResult<()> {
        self.go_on()
    }

    fn PostMoveItem(&self, _flags: u32, _item: Ref<'_, IShellItem>, _folder: Ref<'_, IShellItem>, _new_name: &PCWSTR, _hr: HRESULT, _created: Ref<'_, IShellItem>) -> WinResult<()> {
        Ok(())
    }

    fn PreCopyItem(&self, _flags: u32, _item: Ref<'_, IShellItem>, _folder: Ref<'_, IShellItem>, _new_name: &PCWSTR) -> WinResult<()> {
        self.go_on()
    }

    fn PostCopyItem(&self, _flags: u32, _item: Ref<'_, IShellItem>, _folder: Ref<'_, IShellItem>, _new_name: &PCWSTR, _hr: HRESULT, _created: Ref<'_, IShellItem>) -> WinResult<()> {
        Ok(())
    }

    fn PreDeleteItem(&self, _flags: u32, _item: Ref<'_, IShellItem>) -> WinResult<()> {
        self.go_on()
    }

    fn PostDeleteItem(&self, _flags: u32, _item: Ref<'_, IShellItem>, _hr: HRESULT, _created: Ref<'_, IShellItem>) -> WinResult<()> {
        self.go_on()
    }

    fn PreNewItem(&self, _flags: u32, _folder: Ref<'_, IShellItem>, _new_name: &PCWSTR) -> WinResult<()> {
        self.go_on()
    }

    fn PostNewItem(&self, _flags: u32, _folder: Ref<'_, IShellItem>, _new_name: &PCWSTR, _template: &PCWSTR, _attributes: u32, _hr: HRESULT, _item: Ref<'_, IShellItem>) -> WinResult<()> {
        Ok(())
    }

    /// The shell's own work units: what's done of the total.
    fn UpdateProgress(&self, total: u32, so_far: u32) -> WinResult<()> {
        self.progress.total_items.store(u64::from(total), Ordering::Relaxed);
        self.progress.done_items.store(u64::from(so_far), Ordering::Relaxed);
        self.progress.counted.store(total > 0, Ordering::Relaxed);
        self.go_on()
    }

    fn ResetTimer(&self) -> WinResult<()> {
        Ok(())
    }

    fn PauseTimer(&self) -> WinResult<()> {
        Ok(())
    }

    fn ResumeTimer(&self) -> WinResult<()> {
        Ok(())
    }
}

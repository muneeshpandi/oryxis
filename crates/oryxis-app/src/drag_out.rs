//! Dragging files OUT of the window (issue #167, the drag-out half).
//!
//! The OS model deliberately hides the drop destination from the
//! source: the target (Explorer, another app) receives a data OFFER
//! and performs the copy itself. So the payload is not a path we
//! resolve, it is data we serve:
//!
//! - A LOCAL browser row offers `CF_HDROP` (real paths); the target
//!   copies the files, the classic drag.
//! - A REMOTE (SFTP) row offers VIRTUAL FILES
//!   (`CFSTR_FILEDESCRIPTORW` + `CFSTR_FILECONTENTS`): the drop pulls
//!   an `IStream` we feed from the live SFTP channel, so the download
//!   happens AT DROP TIME straight into wherever the user dropped,
//!   with no temp staging and no size limit. The data object declares
//!   `IDataObjectAsyncCapability`, so Explorer performs that copy on a
//!   background thread after the drag gesture ends instead of inside
//!   the drag's modal loop.
//!
//! Windows only today. macOS wants `NSFilePromiseProvider` (blocked on
//! hardware for QA), X11 wants an XDND source (own protocol work),
//! Wayland wants a winit-fork patch to expose the pointer serial; the
//! gesture plumbing here is platform-neutral so those backends slot in
//! behind [`supported`].
//!
//! Threading: `start` runs on the UI thread (via `iced::window::run`)
//! and `DoDragDrop` blocks it for the duration of the gesture, pumping
//! COM inside; iced stops redrawing meanwhile, which is acceptable for
//! a drag that is mostly happening OVER OTHER WINDOWS. The streams are
//! read by the target's threads afterwards; they bridge into tokio via
//! the runtime handle captured in [`prepare`].

use std::sync::Arc;

/// A drag armed by a row press. The gesture has THREE moments, and
/// keeping them apart is what makes it feel like a drag:
///
/// 1. the press anchors it ([`DragOutStage::Armed`]);
/// 2. the movement threshold resolves the payload (one `open` round
///    trip per remote file) and raises OUR OWN ghost, so the user sees
///    something following the cursor while still over the window;
/// 3. the cursor LEAVING THE WINDOW hands the gesture to the OS.
///
/// Steps 2 and 3 are deliberately not the same moment. `start` blocks
/// the UI thread for the whole OS gesture (see the module docs), so a
/// drag escalated at the threshold freezes the window before a ghost
/// could paint even once, and inside our own window the OS has nothing
/// to show either. Resolving early and escalating late also keeps the
/// round trip off the critical path: a quick flick out of the window
/// finds the payload already prepared instead of racing the button
/// release, which would hand `DoDragDrop` a button that is already up.
#[derive(Debug)]
pub(crate) struct DragOutArm {
    /// Cursor position at the press, the threshold's anchor.
    pub press: iced::Point,
    /// What the ghost says while the drag is still over the window.
    pub label: String,
    pub stage: DragOutStage,
}

#[derive(Debug)]
pub(crate) enum DragOutStage {
    /// Pressed, below the movement threshold. Still a click.
    Armed(DragOutPayload),
    /// Threshold crossed: the ghost is up and `prepare` is in flight.
    Resolving,
    /// Prepared, waiting for the cursor to leave the window.
    Ready(Prepared),
}

impl DragOutArm {
    /// Whether the gesture has become a drag (ghost visible, payload
    /// resolving or resolved). Below the threshold it is still a click.
    pub fn dragging(&self) -> bool {
        !matches!(self.stage, DragOutStage::Armed(_))
    }
}

/// What the armed row offers.
#[derive(Debug, Clone)]
pub(crate) enum DragOutPayload {
    /// Real files on this machine (the local sidebar browser).
    Local(Vec<std::path::PathBuf>),
    /// Remote files served over the pane's SFTP channel.
    Remote {
        client: oryxis_ssh::SftpClient,
        files: Vec<RemoteDragFile>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct RemoteDragFile {
    /// Absolute remote path.
    pub path: String,
    /// The name the dropped file gets on the other side.
    pub name: String,
    pub size: u64,
}

/// Movement (px) between press and cursor that turns a press into a
/// drag. Matches the usual desktop threshold; below it a press is a
/// click (select), which already fired.
pub(crate) const DRAG_THRESHOLD: f32 = 8.0;

/// Whether this build can start an OS drag at all. The gesture never
/// arms where it cannot finish.
pub(crate) fn supported() -> bool {
    cfg!(target_os = "windows")
}

/// Everything [`start`] needs, resolved OFF the UI thread: remote
/// files are stat'd (which pins the size the descriptor promises the
/// target, and answers whether the path is still there) and the tokio
/// handle is captured for the streams' bridge.
///
/// The ranged read handles are deliberately NOT opened here. Each one
/// costs its own SFTP channel, and a selection of a dozen files would
/// open a dozen at once against an `sshd` whose stock `MaxSessions` is
/// 10 -- to serve a target that then copies the files ONE AT A TIME.
/// The streams open on their first read instead, so the channel count
/// follows what is actually being copied.
#[derive(Debug, Clone)]
// The fields are only READ by a platform backend; on the others the
// payload flows through `start`'s unsupported arm untouched.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) enum Prepared {
    Local(Vec<std::path::PathBuf>),
    Remote {
        rt: tokio::runtime::Handle,
        client: oryxis_ssh::SftpClient,
        /// (file name offered to the target, size, absolute remote path).
        files: Vec<(String, u64, String)>,
    },
}

/// Resolve a payload into [`Prepared`]. Runs inside the tokio runtime
/// (a `Task::perform` future), while the mouse button is still down,
/// so it must stay quick: one `stat` round trip per file, all on the
/// pane's EXISTING channel.
pub(crate) async fn prepare(payload: DragOutPayload) -> Result<Prepared, String> {
    match payload {
        DragOutPayload::Local(paths) => Ok(Prepared::Local(paths)),
        DragOutPayload::Remote { client, files } => {
            let rt = tokio::runtime::Handle::current();
            let mut resolved = Vec::with_capacity(files.len());
            for f in files {
                // The listing's size can be stale, and the descriptor's
                // size is what the target sizes its copy against, so it
                // is re-read here. Failing now is also the only chance
                // to say so with a toast: past this point the gesture
                // belongs to the OS, where a vanished file can only
                // surface as a failed copy.
                let stat = client.stat(&f.path).await.map_err(|e| e.to_string())?;
                resolved.push((f.name, stat.size.max(f.size), f.path));
            }
            Ok(Prepared::Remote {
                rt,
                client,
                files: resolved,
            })
        }
    }
}

/// Start the OS drag. Must run on the UI thread with the mouse button
/// still down (`iced::window::run` is the road there). Blocks for the
/// duration of the gesture. `Ok(())` means the drag ran (dropped OR
/// cancelled, both are user outcomes); `Err` means it never started.
#[cfg(target_os = "windows")]
pub(crate) fn start(window: &dyn iced::Window, prepared: Prepared) -> Result<(), String> {
    use iced::window::raw_window_handle::RawWindowHandle;
    let wh = window
        .window_handle()
        .map_err(|e| format!("window handle: {e}"))?;
    let RawWindowHandle::Win32(_) = wh.as_raw() else {
        return Err("not a win32 window".into());
    };
    imp::do_drag(prepared)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn start(_window: &dyn iced::Window, _prepared: Prepared) -> Result<(), String> {
    // The gesture only arms where `supported()` says yes, so this is a
    // routing bug, not a user-visible state.
    Err("drag-out is not supported on this platform yet".into())
}

/// Hop onto the UI thread and run [`start`] there, the only thread that
/// may own an OS drag. The button may already be up by the time this
/// lands (a very quick flick): the drop source then ends the drag on its
/// first `QueryContinueDrag`, which is the right outcome.
pub(crate) fn launch(prepared: Prepared) -> iced::Task<crate::messages::Message> {
    // `window::run` takes an `Fn` closure, so the one-shot payload
    // travels in a take-once slot.
    let slot = Arc::new(std::sync::Mutex::new(Some(prepared)));
    crate::app::main_window()
        .and_then(move |id| {
            let slot = slot.clone();
            iced::window::run(id, move |window| {
                let Some(prepared) = slot.lock().ok().and_then(|mut s| s.take()) else {
                    return;
                };
                if let Err(e) = start(window, prepared) {
                    tracing::warn!("drag-out failed to start: {e}");
                }
            })
        })
        .discard()
}

#[cfg(target_os = "windows")]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    use windows::core::implement;
    use windows::Win32::Foundation::{
        DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC,
        DV_E_LINDEX, DV_E_TYMED, E_NOTIMPL, HGLOBAL, OLE_E_ADVISENOTSUPPORTED, S_FALSE, S_OK,
    };
    use windows::Win32::System::Com::{
        IAdviseSink, IBindCtx, IDataObject, IDataObject_Impl, IEnumFORMATETC,
        IEnumFORMATETC_Impl, IEnumSTATDATA, ISequentialStream_Impl, IStream, IStream_Impl,
        DATADIR_GET, FORMATETC, LOCKTYPE, STATFLAG, STATSTG, STGC, STGMEDIUM, STGMEDIUM_0,
        STREAM_SEEK, TYMED, TYMED_HGLOBAL, TYMED_ISTREAM,
    };
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::{
        DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, CF_HDROP, DROPEFFECT,
        DROPEFFECT_COPY,
    };
    use windows::Win32::System::SystemServices::{MODIFIERKEYS_FLAGS, MK_LBUTTON};
    use windows::Win32::UI::Shell::{
        DROPFILES, FD_ATTRIBUTES, FD_FILESIZE, FD_PROGRESSUI, FILEDESCRIPTORW,
        FILEGROUPDESCRIPTORW, IDataObjectAsyncCapability, IDataObjectAsyncCapability_Impl,
    };
    use windows_core::{Ref, Result as WinResult, BOOL, HRESULT, PCWSTR};

    use super::Prepared;

    /// Per-thread OLE init. winit initializes OLE on the UI thread for
    /// its own drop TARGET; a second `OleInitialize` answers `S_FALSE`
    /// (already initialized), which is fine. We never uninitialize:
    /// the thread lives as long as the window.
    fn ensure_ole() {
        thread_local! {
            static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        }
        DONE.with(|done| {
            if !done.get() {
                // S_OK, S_FALSE and RPC_E_CHANGED_MODE all mean "COM is
                // up on this thread"; only record that we asked.
                let _ = unsafe { OleInitialize(None) };
                done.set(true);
            }
        });
    }

    pub(super) fn do_drag(prepared: Prepared) -> Result<(), String> {
        ensure_ole();
        let data: IDataObject = match prepared {
            Prepared::Local(paths) => DataObject::new_local(paths)?.into(),
            Prepared::Remote { rt, client, files } => {
                DataObject::new_remote(rt, client, files)?.into()
            }
        };
        let source: IDropSource = DropSource.into();
        let mut effect = DROPEFFECT(0);
        // Returns DRAGDROP_S_DROP / DRAGDROP_S_CANCEL (both fine) or a
        // real error. Blocks through the gesture; see module docs.
        let hr = unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect) };
        if hr == DRAGDROP_S_DROP || hr == DRAGDROP_S_CANCEL || hr == S_OK || hr == S_FALSE {
            Ok(())
        } else {
            Err(format!("DoDragDrop: {hr}"))
        }
    }

    /// The standard drop source: Escape cancels, releasing the left
    /// button drops, default cursors throughout.
    #[implement(IDropSource)]
    struct DropSource;

    impl IDropSource_Impl for DropSource_Impl {
        fn QueryContinueDrag(
            &self,
            fescapepressed: BOOL,
            grfkeystate: MODIFIERKEYS_FLAGS,
        ) -> HRESULT {
            if fescapepressed.as_bool() {
                return DRAGDROP_S_CANCEL;
            }
            if (grfkeystate & MK_LBUTTON) == MODIFIERKEYS_FLAGS(0) {
                return DRAGDROP_S_DROP;
            }
            S_OK
        }

        fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }

    /// What one offered format renders to.
    enum Render {
        /// An HGLOBAL built once per GetData call.
        Global(Vec<u8>),
        /// The virtual-file stream for `lindex` N.
        Stream(usize),
    }

    struct Format {
        cf: u16,
        tymed: TYMED,
        /// `-1` for whole-object formats; the file index for
        /// CFSTR_FILECONTENTS, which is offered once per file.
        lindex: i32,
    }

    /// The drag payload as COM sees it. One instance per drag; the
    /// target clones interface pointers, not the object.
    #[implement(IDataObject, IDataObjectAsyncCapability)]
    struct DataObject {
        formats: Vec<Format>,
        /// Pre-serialized HGLOBAL payloads keyed like `formats`
        /// (HDROP bytes / descriptor bytes).
        globals: std::collections::HashMap<u16, Vec<u8>>,
        /// What CFSTR_FILECONTENTS serves, by lindex: (size, remote
        /// path). The read handle behind each one is opened by the
        /// stream itself, on its first read.
        streams: Vec<(u64, String)>,
        /// The channel the remote streams open against. `None` for a
        /// local payload, which serves CF_HDROP and no streams.
        client: Option<oryxis_ssh::SftpClient>,
        rt: Option<tokio::runtime::Handle>,
        /// IDataObjectAsyncCapability bookkeeping.
        async_mode: AtomicBool,
        in_operation: AtomicBool,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn cfstr(name: &str) -> u16 {
        let mut w = wide(name);
        w.push(0);
        (unsafe { RegisterClipboardFormatW(PCWSTR(w.as_ptr())) }) as u16
    }

    impl DataObject {
        /// Real local paths: one CF_HDROP with the classic DROPFILES +
        /// double-NUL-terminated wide path list.
        fn new_local(paths: Vec<std::path::PathBuf>) -> Result<Self, String> {
            if paths.is_empty() {
                return Err("nothing to drag".into());
            }
            let mut list: Vec<u16> = Vec::new();
            for p in &paths {
                list.extend(p.as_os_str().to_string_lossy().encode_utf16());
                list.push(0);
            }
            list.push(0);
            let header_len = std::mem::size_of::<DROPFILES>();
            let mut bytes = vec![0u8; header_len + list.len() * 2];
            let df = DROPFILES {
                pFiles: header_len as u32,
                fWide: true.into(),
                ..Default::default()
            };
            unsafe {
                std::ptr::copy_nonoverlapping(
                    (&df as *const DROPFILES).cast::<u8>(),
                    bytes.as_mut_ptr(),
                    header_len,
                );
                std::ptr::copy_nonoverlapping(
                    list.as_ptr().cast::<u8>(),
                    bytes.as_mut_ptr().add(header_len),
                    list.len() * 2,
                );
            }
            let mut globals = std::collections::HashMap::new();
            globals.insert(CF_HDROP.0, bytes);
            Ok(Self {
                formats: vec![Format { cf: CF_HDROP.0, tymed: TYMED_HGLOBAL, lindex: -1 }],
                globals,
                streams: Vec::new(),
                client: None,
                rt: None,
                async_mode: AtomicBool::new(true),
                in_operation: AtomicBool::new(false),
            })
        }

        /// Remote files: FILEGROUPDESCRIPTORW naming them (with sizes,
        /// so the target shows real progress) + one FILECONTENTS
        /// stream per index, pulled at drop time.
        fn new_remote(
            rt: tokio::runtime::Handle,
            client: oryxis_ssh::SftpClient,
            files: Vec<(String, u64, String)>,
        ) -> Result<Self, String> {
            if files.is_empty() {
                return Err("nothing to drag".into());
            }
            let cf_descriptor = cfstr("FileGroupDescriptorW");
            let cf_contents = cfstr("FileContents");

            // FILEGROUPDESCRIPTORW is declared with a 1-element array;
            // the real payload is cItems descriptors back to back.
            let head = std::mem::size_of::<FILEGROUPDESCRIPTORW>()
                - std::mem::size_of::<FILEDESCRIPTORW>();
            let mut bytes =
                vec![0u8; head + files.len() * std::mem::size_of::<FILEDESCRIPTORW>()];
            let count = files.len() as u32;
            unsafe {
                std::ptr::copy_nonoverlapping(
                    (&count as *const u32).cast::<u8>(),
                    bytes.as_mut_ptr(),
                    4,
                );
            }
            for (i, (name, size, _)) in files.iter().enumerate() {
                let mut fd = FILEDESCRIPTORW {
                    dwFlags: (FD_FILESIZE.0 | FD_ATTRIBUTES.0 | FD_PROGRESSUI.0) as u32,
                    dwFileAttributes: 0x80, // FILE_ATTRIBUTE_NORMAL
                    nFileSizeHigh: (size >> 32) as u32,
                    nFileSizeLow: (size & 0xFFFF_FFFF) as u32,
                    ..Default::default()
                };
                // Sanitize the remote-supplied name into one path
                // component; a separator here would let a host write
                // outside the drop folder.
                let safe: String = name
                    .chars()
                    .map(|c| match c {
                        '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                        c if (c as u32) < 0x20 => '_',
                        c => c,
                    })
                    .collect();
                for (j, u) in wide(&safe).into_iter().take(259).enumerate() {
                    fd.cFileName[j] = u;
                }
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        (&fd as *const FILEDESCRIPTORW).cast::<u8>(),
                        bytes
                            .as_mut_ptr()
                            .add(head + i * std::mem::size_of::<FILEDESCRIPTORW>()),
                        std::mem::size_of::<FILEDESCRIPTORW>(),
                    );
                }
            }

            let mut formats =
                vec![Format { cf: cf_descriptor, tymed: TYMED_HGLOBAL, lindex: -1 }];
            for i in 0..files.len() {
                formats.push(Format {
                    cf: cf_contents,
                    tymed: TYMED_ISTREAM,
                    lindex: i as i32,
                });
            }
            let mut globals = std::collections::HashMap::new();
            globals.insert(cf_descriptor, bytes);
            Ok(Self {
                formats,
                globals,
                streams: files.into_iter().map(|(_, s, p)| (s, p)).collect(),
                client: Some(client),
                rt: Some(rt),
                async_mode: AtomicBool::new(true),
                in_operation: AtomicBool::new(false),
            })
        }

        fn render(&self, fmt: &FORMATETC) -> Result<Render, HRESULT> {
            let Some(spec) = self.formats.iter().find(|f| f.cf == fmt.cfFormat) else {
                return Err(DV_E_FORMATETC);
            };
            if (fmt.tymed & spec.tymed.0 as u32) == 0 {
                return Err(DV_E_TYMED);
            }
            match spec.tymed {
                TYMED_HGLOBAL => self
                    .globals
                    .get(&spec.cf)
                    .map(|b| Render::Global(b.clone()))
                    .ok_or(DV_E_FORMATETC),
                TYMED_ISTREAM => {
                    let idx = fmt.lindex;
                    if idx < 0 || idx as usize >= self.streams.len() {
                        return Err(DV_E_LINDEX);
                    }
                    Ok(Render::Stream(idx as usize))
                }
                _ => Err(DV_E_TYMED),
            }
        }
    }

    fn to_hglobal(bytes: &[u8]) -> WinResult<HGLOBAL> {
        unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
            let p = GlobalLock(h);
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.cast::<u8>(), bytes.len());
            let _ = GlobalUnlock(h);
            Ok(h)
        }
    }

    impl IDataObject_Impl for DataObject_Impl {
        fn GetData(&self, pformatetcin: *const FORMATETC) -> WinResult<STGMEDIUM> {
            let fmt = unsafe { &*pformatetcin };
            match self.render(fmt) {
                Ok(Render::Global(bytes)) => {
                    let h = to_hglobal(&bytes)?;
                    Ok(STGMEDIUM {
                        tymed: TYMED_HGLOBAL.0 as u32,
                        u: STGMEDIUM_0 { hGlobal: h },
                        pUnkForRelease: std::mem::ManuallyDrop::new(None),
                    })
                }
                Ok(Render::Stream(idx)) => {
                    let (size, path) = &self.streams[idx];
                    let rt = self.rt.clone().expect("remote payload carries a runtime");
                    let client = self
                        .client
                        .clone()
                        .expect("remote payload carries a client");
                    let stream: IStream = RemoteStream {
                        rt,
                        client,
                        path: path.clone(),
                        size: *size,
                        pos64: Mutex::new(0),
                        file: Mutex::new(None),
                    }
                    .into();
                    Ok(STGMEDIUM {
                        tymed: TYMED_ISTREAM.0 as u32,
                        u: STGMEDIUM_0 {
                            pstm: std::mem::ManuallyDrop::new(Some(stream)),
                        },
                        pUnkForRelease: std::mem::ManuallyDrop::new(None),
                    })
                }
                Err(hr) => Err(hr.into()),
            }
        }

        fn GetDataHere(
            &self,
            _pformatetc: *const FORMATETC,
            _pmedium: *mut STGMEDIUM,
        ) -> WinResult<()> {
            Err(E_NOTIMPL.into())
        }

        fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
            let fmt = unsafe { &*pformatetc };
            match self.render(fmt) {
                Ok(_) => S_OK,
                Err(hr) => hr,
            }
        }

        fn GetCanonicalFormatEtc(
            &self,
            _pformatectin: *const FORMATETC,
            pformatetcout: *mut FORMATETC,
        ) -> HRESULT {
            unsafe {
                (*pformatetcout).ptd = std::ptr::null_mut();
            }
            // DATA_S_SAMEFORMATETC without dragging the constant in.
            HRESULT(0x0004_0130)
        }

        fn SetData(
            &self,
            _pformatetc: *const FORMATETC,
            _pmedium: *const STGMEDIUM,
            _frelease: BOOL,
        ) -> WinResult<()> {
            // The shell stamps "Performed DropEffect" and friends here.
            // We have nothing to do with them, but refusing makes some
            // targets abort, so accept-and-ignore.
            Ok(())
        }

        fn EnumFormatEtc(&self, dwdirection: u32) -> WinResult<IEnumFORMATETC> {
            if dwdirection != DATADIR_GET.0 as u32 {
                return Err(E_NOTIMPL.into());
            }
            let list: Vec<FORMATETC> = self
                .formats
                .iter()
                .map(|f| FORMATETC {
                    cfFormat: f.cf,
                    ptd: std::ptr::null_mut(),
                    dwAspect: 1, // DVASPECT_CONTENT
                    lindex: f.lindex,
                    tymed: f.tymed.0 as u32,
                })
                .collect();
            Ok(EnumFormats { list, pos: AtomicU32::new(0) }.into())
        }

        fn DAdvise(
            &self,
            _pformatetc: *const FORMATETC,
            _advf: u32,
            _padvsink: Ref<'_, IAdviseSink>,
        ) -> WinResult<u32> {
            Err(OLE_E_ADVISENOTSUPPORTED.into())
        }

        fn DUnadvise(&self, _dwconnection: u32) -> WinResult<()> {
            Err(OLE_E_ADVISENOTSUPPORTED.into())
        }

        fn EnumDAdvise(&self) -> WinResult<IEnumSTATDATA> {
            Err(OLE_E_ADVISENOTSUPPORTED.into())
        }
    }

    impl IDataObjectAsyncCapability_Impl for DataObject_Impl {
        fn SetAsyncMode(&self, fdoopasync: BOOL) -> WinResult<()> {
            self.async_mode.store(fdoopasync.as_bool(), Ordering::Relaxed);
            Ok(())
        }

        fn GetAsyncMode(&self) -> WinResult<BOOL> {
            Ok(self.async_mode.load(Ordering::Relaxed).into())
        }

        fn StartOperation(&self, _pbcreserved: Ref<'_, IBindCtx>) -> WinResult<()> {
            self.in_operation.store(true, Ordering::Relaxed);
            Ok(())
        }

        fn InOperation(&self) -> WinResult<BOOL> {
            Ok(self.in_operation.load(Ordering::Relaxed).into())
        }

        fn EndOperation(
            &self,
            _hresult: HRESULT,
            _pbcreserved: Ref<'_, IBindCtx>,
            _dweffects: u32,
        ) -> WinResult<()> {
            self.in_operation.store(false, Ordering::Relaxed);
            Ok(())
        }
    }

    /// The enumerator `EnumFormatEtc` hands out; the target walks it
    /// once.
    #[implement(IEnumFORMATETC)]
    struct EnumFormats {
        list: Vec<FORMATETC>,
        pos: AtomicU32,
    }

    impl IEnumFORMATETC_Impl for EnumFormats_Impl {
        fn Next(
            &self,
            celt: u32,
            rgelt: *mut FORMATETC,
            pceltfetched: *mut u32,
        ) -> HRESULT {
            let mut served = 0u32;
            let mut pos = self.pos.load(Ordering::Relaxed) as usize;
            while served < celt && pos < self.list.len() {
                unsafe {
                    *rgelt.add(served as usize) = self.list[pos];
                }
                pos += 1;
                served += 1;
            }
            self.pos.store(pos as u32, Ordering::Relaxed);
            if !pceltfetched.is_null() {
                unsafe {
                    *pceltfetched = served;
                }
            }
            if served == celt {
                S_OK
            } else {
                S_FALSE
            }
        }

        fn Skip(&self, celt: u32) -> WinResult<()> {
            let pos = self.pos.load(Ordering::Relaxed).saturating_add(celt);
            let capped = pos.min(self.list.len() as u32);
            self.pos.store(capped, Ordering::Relaxed);
            if pos == capped {
                Ok(())
            } else {
                Err(S_FALSE.into())
            }
        }

        fn Reset(&self) -> WinResult<()> {
            self.pos.store(0, Ordering::Relaxed);
            Ok(())
        }

        fn Clone(&self) -> WinResult<IEnumFORMATETC> {
            Ok(EnumFormats {
                list: self.list.clone(),
                pos: AtomicU32::new(self.pos.load(Ordering::Relaxed)),
            }
            .into())
        }
    }

    /// The virtual file's byte source: sequential reads bridged into
    /// the SFTP ranged handle through the captured tokio runtime. The
    /// target reads from its own thread (async data object), so the
    /// `block_on` here parks a shell worker, never our UI thread.
    ///
    /// The handle behind it opens on the FIRST read, not at drag time:
    /// it costs an SFTP channel of its own, the target copies the files
    /// one after another, and a data object offers one stream per file.
    /// Opening them all up front would spend a dozen channels to serve
    /// one copy (see [`Prepared`]).
    #[implement(IStream)]
    struct RemoteStream {
        rt: tokio::runtime::Handle,
        client: oryxis_ssh::SftpClient,
        path: String,
        size: u64,
        /// Read cursor. A plain mutex: the shell reads sequentially
        /// from one thread; Clone snapshots it.
        pos64: Mutex<u64>,
        /// Opened on first use and kept for the rest of the copy.
        file: Mutex<Option<Arc<oryxis_ssh::RemoteRangedFile>>>,
    }

    impl RemoteStream {
        /// The read handle, opening it if this is the first read.
        /// Every caller is already on a target thread, so the open
        /// blocks a shell worker like the reads do.
        fn handle(&self) -> Result<Arc<oryxis_ssh::RemoteRangedFile>, HRESULT> {
            const E_FAIL: HRESULT = HRESULT(0x8000_4005u32 as i32);
            let mut slot = self.file.lock().map_err(|_| E_FAIL)?;
            if let Some(f) = slot.as_ref() {
                return Ok(f.clone());
            }
            match self.rt.block_on(self.client.open_ranged(&self.path)) {
                Ok(f) => {
                    let f = Arc::new(f);
                    *slot = Some(f.clone());
                    Ok(f)
                }
                Err(e) => {
                    tracing::warn!("drag-out could not open {}: {e}", self.path);
                    Err(E_FAIL)
                }
            }
        }
    }

    impl ISequentialStream_Impl for RemoteStream_Impl {
        fn Read(&self, pv: *mut core::ffi::c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
            let file = match self.handle() {
                Ok(f) => f,
                Err(hr) => return hr,
            };
            let mut pos = match self.pos64.lock() {
                Ok(g) => g,
                Err(_) => return HRESULT(0x8000_4005u32 as i32), // E_FAIL
            };
            let want = (cb as u64).min(self.size.saturating_sub(*pos)) as usize;
            let mut done = 0usize;
            while done < want {
                let chunk = self
                    .rt
                    .block_on(file.read_at(*pos + done as u64, want - done));
                match chunk {
                    Ok(bytes) if bytes.is_empty() => break,
                    Ok(bytes) => {
                        unsafe {
                            std::ptr::copy_nonoverlapping(
                                bytes.as_ptr(),
                                pv.cast::<u8>().add(done),
                                bytes.len(),
                            );
                        }
                        done += bytes.len();
                    }
                    Err(e) => {
                        tracing::warn!("drag-out stream read failed: {e}");
                        return HRESULT(0x8000_4005u32 as i32); // E_FAIL
                    }
                }
            }
            *pos += done as u64;
            if !pcbread.is_null() {
                unsafe {
                    *pcbread = done as u32;
                }
            }
            // S_FALSE = fewer bytes than asked (EOF), per the contract.
            if done == cb as usize { S_OK } else { S_FALSE }
        }

        fn Write(
            &self,
            _pv: *const core::ffi::c_void,
            _cb: u32,
            _pcbwritten: *mut u32,
        ) -> HRESULT {
            E_NOTIMPL
        }
    }

    impl IStream_Impl for RemoteStream_Impl {
        fn Seek(
            &self,
            dlibmove: i64,
            dworigin: STREAM_SEEK,
            plibnewposition: *mut u64,
        ) -> WinResult<()> {
            let mut pos = self
                .pos64
                .lock()
                .map_err(|_| windows_core::Error::from(E_NOTIMPL))?;
            let base = match dworigin.0 {
                0 => 0i128,             // STREAM_SEEK_SET
                1 => *pos as i128,      // STREAM_SEEK_CUR
                2 => self.size as i128, // STREAM_SEEK_END
                _ => return Err(E_NOTIMPL.into()),
            };
            let target = base + dlibmove as i128;
            if target < 0 {
                return Err(E_NOTIMPL.into());
            }
            *pos = target as u64;
            if !plibnewposition.is_null() {
                unsafe {
                    *plibnewposition = *pos;
                }
            }
            Ok(())
        }

        fn SetSize(&self, _libnewsize: u64) -> WinResult<()> {
            Err(E_NOTIMPL.into())
        }

        fn CopyTo(
            &self,
            _pstm: Ref<'_, IStream>,
            _cb: u64,
            _pcbread: *mut u64,
            _pcbwritten: *mut u64,
        ) -> WinResult<()> {
            Err(E_NOTIMPL.into())
        }

        fn Commit(&self, _grfcommitflags: &STGC) -> WinResult<()> {
            Ok(())
        }

        fn Revert(&self) -> WinResult<()> {
            Err(E_NOTIMPL.into())
        }

        fn LockRegion(&self, _liboffset: u64, _cb: u64, _dwlocktype: &LOCKTYPE) -> WinResult<()> {
            Err(E_NOTIMPL.into())
        }

        fn UnlockRegion(&self, _liboffset: u64, _cb: u64, _dwlocktype: u32) -> WinResult<()> {
            Err(E_NOTIMPL.into())
        }

        fn Stat(&self, pstatstg: *mut STATSTG, _grfstatflag: &STATFLAG) -> WinResult<()> {
            unsafe {
                let st = &mut *pstatstg;
                *st = Default::default();
                st.r#type = 2; // STGTY_STREAM
                st.cbSize = self.size;
            }
            Ok(())
        }

        fn Clone(&self) -> WinResult<IStream> {
            let pos = self.pos64.lock().map(|g| *g).unwrap_or(0);
            // The clone shares the OPEN handle when there is one (reads
            // are positioned, so one handle serves both cursors) and
            // opens its own on first use otherwise.
            let file = self.file.lock().map(|g| g.clone()).unwrap_or(None);
            Ok(RemoteStream {
                rt: self.rt.clone(),
                client: self.client.clone(),
                path: self.path.clone(),
                size: self.size,
                pos64: Mutex::new(pos),
                file: Mutex::new(file),
            }
            .into())
        }
    }
}

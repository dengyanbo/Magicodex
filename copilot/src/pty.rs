//! A Windows pseudo console that hosts the child Copilot CLI, and the real console around it.

use std::ffi::OsStr;
use std::ffi::OsString;
use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::path::PathBuf;
use std::ptr;
use std::sync::mpsc::Sender;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Foundation::S_OK;
use windows_sys::Win32::System::Console::CONSOLE_MODE;
use windows_sys::Win32::System::Console::CONSOLE_SCREEN_BUFFER_INFO;
use windows_sys::Win32::System::Console::COORD;
use windows_sys::Win32::System::Console::DISABLE_NEWLINE_AUTO_RETURN;
use windows_sys::Win32::System::Console::ENABLE_EXTENDED_FLAGS;
use windows_sys::Win32::System::Console::ENABLE_PROCESSED_OUTPUT;
use windows_sys::Win32::System::Console::ENABLE_VIRTUAL_TERMINAL_INPUT;
use windows_sys::Win32::System::Console::ENABLE_VIRTUAL_TERMINAL_PROCESSING;
use windows_sys::Win32::System::Console::ENABLE_WINDOW_INPUT;
use windows_sys::Win32::System::Console::FOCUS_EVENT;
use windows_sys::Win32::System::Console::GetConsoleCP;
use windows_sys::Win32::System::Console::GetConsoleMode;
use windows_sys::Win32::System::Console::GetConsoleOutputCP;
use windows_sys::Win32::System::Console::GetConsoleScreenBufferInfo;
use windows_sys::Win32::System::Console::GetStdHandle;
use windows_sys::Win32::System::Console::HPCON;
use windows_sys::Win32::System::Console::INPUT_RECORD;
use windows_sys::Win32::System::Console::KEY_EVENT;
use windows_sys::Win32::System::Console::ReadConsoleInputW;
use windows_sys::Win32::System::Console::STD_INPUT_HANDLE;
use windows_sys::Win32::System::Console::STD_OUTPUT_HANDLE;
use windows_sys::Win32::System::Console::SetConsoleCP;
use windows_sys::Win32::System::Console::SetConsoleMode;
use windows_sys::Win32::System::Console::SetConsoleOutputCP;
use windows_sys::Win32::System::Console::WINDOW_BUFFER_SIZE_EVENT;
use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
use windows_sys::Win32::System::LibraryLoader::LOAD_WITH_ALTERED_SEARCH_PATH;
use windows_sys::Win32::System::LibraryLoader::LoadLibraryExW;
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::CREATE_UNICODE_ENVIRONMENT;
use windows_sys::Win32::System::Threading::CreateProcessW;
use windows_sys::Win32::System::Threading::DeleteProcThreadAttributeList;
use windows_sys::Win32::System::Threading::EXTENDED_STARTUPINFO_PRESENT;
use windows_sys::Win32::System::Threading::GetExitCodeProcess;
use windows_sys::Win32::System::Threading::INFINITE;
use windows_sys::Win32::System::Threading::InitializeProcThreadAttributeList;
use windows_sys::Win32::System::Threading::LPPROC_THREAD_ATTRIBUTE_LIST;
use windows_sys::Win32::System::Threading::PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE;
use windows_sys::Win32::System::Threading::PROCESS_INFORMATION;
use windows_sys::Win32::System::Threading::STARTF_USESTDHANDLES;
use windows_sys::Win32::System::Threading::STARTUPINFOEXW;
use windows_sys::Win32::System::Threading::UpdateProcThreadAttribute;
use windows_sys::Win32::System::Threading::WaitForSingleObject;

use crate::app::Msg;

type CreateFn =
    unsafe extern "system" fn(COORD, HANDLE, HANDLE, u32, *mut HPCON) -> windows_sys::core::HRESULT;
type ResizeFn = unsafe extern "system" fn(HPCON, COORD) -> windows_sys::core::HRESULT;
type CloseFn = unsafe extern "system" fn(HPCON);

/// The pseudo console implementation: Windows Terminal's `conpty.dll` shipped next to this
/// program when present, like Windows Terminal itself uses, or the one built into Windows.
struct Conpty {
    create: CreateFn,
    resize: ResizeFn,
    close: CloseFn,
    sideloaded: bool,
}

fn conpty() -> &'static Conpty {
    static API: std::sync::OnceLock<Conpty> = std::sync::OnceLock::new();
    API.get_or_init(|| {
        sideloaded().unwrap_or(Conpty {
            create: windows_sys::Win32::System::Console::CreatePseudoConsole,
            resize: windows_sys::Win32::System::Console::ResizePseudoConsole,
            close: windows_sys::Win32::System::Console::ClosePseudoConsole,
            sideloaded: false,
        })
    })
}

fn sideloaded() -> Option<Conpty> {
    let dll = std::env::current_exe().ok()?.parent()?.join("conpty.dll");
    if !dll.is_file() || !dll.with_file_name("OpenConsole.exe").is_file() {
        return None;
    }
    let path = wide(dll.as_os_str());
    unsafe {
        let module = LoadLibraryExW(
            path.as_ptr(),
            ptr::null_mut(),
            LOAD_WITH_ALTERED_SEARCH_PATH,
        );
        if module.is_null() {
            return None;
        }
        let create = GetProcAddress(module, c"CreatePseudoConsole".as_ptr().cast())?;
        let resize = GetProcAddress(module, c"ResizePseudoConsole".as_ptr().cast())?;
        let close = GetProcAddress(module, c"ClosePseudoConsole".as_ptr().cast())?;
        Some(Conpty {
            create: std::mem::transmute::<unsafe extern "system" fn() -> isize, CreateFn>(create),
            resize: std::mem::transmute::<unsafe extern "system" fn() -> isize, ResizeFn>(resize),
            close: std::mem::transmute::<unsafe extern "system" fn() -> isize, CloseFn>(close),
            sideloaded: true,
        })
    }
}

/// Whether the pseudo console comes from the `conpty.dll` next to this program.
pub(crate) fn sideloaded_conpty() -> bool {
    conpty().sideloaded
}

/// The pseudo console of the running child and the write end of its input.
pub(crate) struct Pty {
    console: HPCON,
    pid: u32,
}

// The pseudo console handle is only used from the main thread; the type is moved there once.
unsafe impl Send for Pty {}

/// The child process handle, owned by the thread that waits for it to exit.
pub(crate) struct ChildProcess(HANDLE);

unsafe impl Send for ChildProcess {}

impl ChildProcess {
    pub(crate) fn wait(self) -> u32 {
        let mut code = 1;
        unsafe {
            WaitForSingleObject(self.0, INFINITE);
            GetExitCodeProcess(self.0, &mut code);
            CloseHandle(self.0);
        }
        code
    }
}

pub(crate) struct Spawned {
    pub(crate) pty: Pty,
    /// Bytes typed into the child.
    pub(crate) input: File,
    /// Everything the child draws.
    pub(crate) output: File,
    pub(crate) process: ChildProcess,
}

fn pipe() -> io::Result<(HANDLE, HANDLE)> {
    let (mut read, mut write) = (ptr::null_mut(), ptr::null_mut());
    if unsafe { CreatePipe(&mut read, &mut write, ptr::null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((read, write))
}

fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

fn coord((cols, rows): (u16, u16)) -> COORD {
    COORD {
        X: cols.clamp(1, i16::MAX as u16) as i16,
        Y: rows.clamp(1, i16::MAX as u16) as i16,
    }
}

impl Pty {
    pub(crate) fn spawn(
        command_line: &str,
        cwd: Option<&Path>,
        env: &[(OsString, OsString)],
        size: (u16, u16),
    ) -> io::Result<Spawned> {
        let (input_read, input_write) = pipe()?;
        let (output_read, output_write) = match pipe() {
            Ok(pair) => pair,
            Err(err) => {
                unsafe {
                    CloseHandle(input_read);
                    CloseHandle(input_write);
                }
                return Err(err);
            }
        };
        let mut console: HPCON = 0;
        let result =
            unsafe { (conpty().create)(coord(size), input_read, output_write, 0, &mut console) };
        // The pseudo console holds its own duplicates of these ends.
        unsafe {
            CloseHandle(input_read);
            CloseHandle(output_write);
        }
        let input = unsafe { File::from_raw_handle(input_write as _) };
        let output = unsafe { File::from_raw_handle(output_read as _) };
        if result != S_OK {
            return Err(io::Error::other(format!(
                "CreatePseudoConsole failed: HRESULT {result:#010x}"
            )));
        }
        let mut pty = Pty { console, pid: 0 };
        let process = pty.create_process(command_line, cwd, env)?;
        Ok(Spawned {
            pty,
            input,
            output,
            process,
        })
    }

    fn create_process(
        &mut self,
        command_line: &str,
        cwd: Option<&Path>,
        env: &[(OsString, OsString)],
    ) -> io::Result<ChildProcess> {
        let mut size = 0;
        unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size) };
        let mut storage = vec![0_u64; size.div_ceil(8)];
        let list = storage.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut size) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let attached = unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                self.console as *const c_void,
                std::mem::size_of::<HPCON>(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if attached == 0 {
            let err = io::Error::last_os_error();
            unsafe { DeleteProcThreadAttributeList(list) };
            return Err(err);
        }
        let mut info: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
        info.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        // Never let the child pick up redirected standard handles of this process.
        info.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        info.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
        info.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
        info.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
        info.lpAttributeList = list;

        let mut command: Vec<u16> = command_line.encode_utf16().chain([0]).collect();
        let mut block = Vec::new();
        for (name, value) in env {
            block.extend(name.encode_wide());
            block.push(u16::from(b'='));
            block.extend(value.encode_wide());
            block.push(0);
        }
        block.push(0);
        let directory = cwd.map(|path| wide(path.as_os_str()));
        let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let created = unsafe {
            CreateProcessW(
                ptr::null(),
                command.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                block.as_ptr() as *const c_void,
                directory.as_ref().map_or(ptr::null(), |dir| dir.as_ptr()),
                &info.StartupInfo,
                &mut process,
            )
        };
        let err = io::Error::last_os_error();
        unsafe { DeleteProcThreadAttributeList(list) };
        if created == 0 {
            return Err(io::Error::new(
                err.kind(),
                format!("could not start `{command_line}`: {err}"),
            ));
        }
        unsafe { CloseHandle(process.hThread) };
        self.pid = process.dwProcessId;
        Ok(ChildProcess(process.hProcess))
    }

    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn resize(&self, size: (u16, u16)) {
        unsafe { (conpty().resize)(self.console, coord(size)) };
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Closing tells the child's console to shut down; the output reader drains the rest.
        unsafe { (conpty().close)(self.console) };
    }
}

/// The Windows system directory, e.g. `C:\Windows\System32`.
pub(crate) fn system_dir() -> PathBuf {
    let mut buffer = [0_u16; 520];
    let len = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if len == 0 || len > buffer.len() {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        return PathBuf::from(root).join("System32");
    }
    PathBuf::from(OsString::from_wide(&buffer[..len]))
}

/// The command line that runs a batch file through `cmd.exe` so that each argument reaches it
/// literally: no `&`, `|` or `%VAR%` in an argument is interpreted by `cmd.exe`.
///
/// This is the escaping the Rust standard library applies when it starts `.bat`/`.cmd` files
/// (see CVE-2024-24576); arguments with line breaks cannot be passed safely and are refused.
pub(crate) fn batch_command_line(script: &Path, args: &[&OsStr]) -> io::Result<String> {
    let cmd = quote_arg(&system_dir().join("cmd.exe").to_string_lossy());
    Ok(format!("{cmd} {}", batch_arguments(script, args)?))
}

fn batch_arguments(script: &Path, args: &[&OsStr]) -> io::Result<String> {
    let invalid = |message: &str| io::Error::new(io::ErrorKind::InvalidInput, message.to_string());
    let script = script.to_string_lossy();
    if script.contains('"') || script.ends_with('\\') {
        return Err(invalid(
            "batch file names may not contain `\"` or end with `\\`",
        ));
    }
    // The whole command is quoted once more; cmd.exe strips that outer pair.
    let mut line = format!("/e:ON /v:OFF /d /c \"\"{script}\"");
    for arg in args {
        let arg = arg.to_string_lossy();
        if arg.contains(['\r', '\n', '\0']) {
            return Err(invalid(
                "arguments for a Copilot CLI started through a .cmd/.bat file cannot contain line breaks",
            ));
        }
        line.push(' ');
        push_batch_arg(&mut line, &arg);
    }
    line.push('"');
    Ok(line)
}

fn push_batch_arg(line: &mut String, arg: &str) {
    const UNQUOTED: &str = r"#$*+-./:?@\_";
    let quote = arg.is_empty()
        || arg.ends_with('\\')
        || arg.chars().any(|c| {
            (c.is_ascii() && !(c.is_ascii_alphanumeric() || UNQUOTED.contains(c))) || c.is_control()
        });
    if quote {
        line.push('"');
    }
    let mut backslashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            backslashes += 1;
        } else {
            if c == '"' {
                // Backslashes before a quote are doubled, and the quote itself is doubled.
                line.extend(std::iter::repeat_n('\\', backslashes));
                line.push('"');
            } else if c == '%' {
                // `%cd:~,%` expands to nothing, which keeps `%VAR%` from being expanded.
                line.push_str("%%cd:~,");
            }
            backslashes = 0;
        }
        line.push(c);
    }
    if quote {
        line.extend(std::iter::repeat_n('\\', backslashes));
        line.push('"');
    }
}

/// Quotes one argument for `CreateProcessW` following the MSVC runtime rules.
pub(crate) fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\u{b}', '"']) {
        return arg.to_string();
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                quoted.push(c);
                backslashes = 0;
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

/// The real console this process runs in, switched to raw VT input and output.
pub(crate) struct Console {
    input: HANDLE,
    output: HANDLE,
    modes: (CONSOLE_MODE, CONSOLE_MODE),
    code_pages: (u32, u32),
}

impl Console {
    /// Fails when either standard handle is not a console, e.g. when output is piped.
    pub(crate) fn enter() -> io::Result<Self> {
        let (input, output) = unsafe {
            (
                GetStdHandle(STD_INPUT_HANDLE),
                GetStdHandle(STD_OUTPUT_HANDLE),
            )
        };
        let (mut input_mode, mut output_mode) = (0, 0);
        if unsafe { GetConsoleMode(input, &mut input_mode) } == 0
            || unsafe { GetConsoleMode(output, &mut output_mode) } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let console = Self {
            input,
            output,
            modes: (input_mode, output_mode),
            code_pages: unsafe { (GetConsoleCP(), GetConsoleOutputCP()) },
        };
        // Keys arrive as the VT bytes the terminal sends, so they can be forwarded unchanged.
        let raw_input = ENABLE_VIRTUAL_TERMINAL_INPUT | ENABLE_WINDOW_INPUT | ENABLE_EXTENDED_FLAGS;
        let vt_output = ENABLE_PROCESSED_OUTPUT
            | ENABLE_VIRTUAL_TERMINAL_PROCESSING
            | DISABLE_NEWLINE_AUTO_RETURN;
        unsafe {
            if SetConsoleMode(input, raw_input) == 0 || SetConsoleMode(output, vt_output) == 0 {
                return Err(io::Error::last_os_error());
            }
            SetConsoleCP(65001);
            SetConsoleOutputCP(65001);
        }
        Ok(console)
    }

    /// Visible window size as `(columns, rows)`.
    pub(crate) fn size(&self) -> (u16, u16) {
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = unsafe { std::mem::zeroed() };
        if unsafe { GetConsoleScreenBufferInfo(self.output, &mut info) } == 0 {
            return (120, 40);
        }
        let window = info.srWindow;
        (
            (window.Right - window.Left + 1).max(1) as u16,
            (window.Bottom - window.Top + 1).max(1) as u16,
        )
    }

    /// Reads console input records on a thread of their own until the console goes away.
    pub(crate) fn spawn_reader(&self, tx: Sender<Msg>) {
        let handle = self.input as usize;
        std::thread::spawn(move || read_input(handle as HANDLE, &tx));
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        unsafe {
            SetConsoleMode(self.input, self.modes.0);
            SetConsoleMode(self.output, self.modes.1);
            SetConsoleCP(self.code_pages.0);
            SetConsoleOutputCP(self.code_pages.1);
        }
    }
}

fn read_input(handle: HANDLE, tx: &Sender<Msg>) {
    let mut records: [INPUT_RECORD; 128] = unsafe { std::mem::zeroed() };
    let mut high_surrogate = None;
    loop {
        let mut count = 0;
        if unsafe { ReadConsoleInputW(handle, records.as_mut_ptr(), 128, &mut count) } == 0 {
            break;
        }
        let mut units = Vec::new();
        let mut resized = false;
        for record in &records[..count as usize] {
            match u32::from(record.EventType) {
                KEY_EVENT => {
                    let key = unsafe { record.Event.KeyEvent };
                    let unit = unsafe { key.uChar.UnicodeChar };
                    if key.bKeyDown != 0 && unit != 0 {
                        units.extend(std::iter::repeat_n(
                            unit,
                            usize::from(key.wRepeatCount.max(1)),
                        ));
                    }
                }
                WINDOW_BUFFER_SIZE_EVENT => resized = true,
                FOCUS_EVENT => {
                    let focus = unsafe { record.Event.FocusEvent };
                    let _ = tx.send(Msg::Focus(focus.bSetFocus != 0));
                }
                _ => {}
            }
        }
        if let Some(high) = high_surrogate.take() {
            units.insert(0, high);
        }
        if units
            .last()
            .is_some_and(|unit| (0xD800..0xDC00).contains(unit))
        {
            high_surrogate = units.pop();
        }
        if !units.is_empty() {
            let text: String = char::decode_utf16(units)
                .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect();
            if tx.send(Msg::Input(text)).is_err() {
                break;
            }
        }
        if resized && tx.send(Msg::Resize).is_err() {
            break;
        }
    }
}

/// Writes to the child on a thread of its own so a busy child never stalls drawing.
pub(crate) fn spawn_writer(mut input: File) -> Sender<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        for bytes in rx {
            if input.write_all(&bytes).is_err() {
                break;
            }
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::windows::process::CommandExt;
    use std::path::Path;

    use super::batch_arguments;
    use super::batch_command_line;
    use super::quote_arg;
    use super::system_dir;

    #[test]
    fn arguments_are_quoted_for_the_msvc_runtime() {
        assert_eq!(quote_arg("plain"), "plain");
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(quote_arg("two words"), "\"two words\"");
        assert_eq!(quote_arg("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(
            quote_arg("C:\\dir with space\\"),
            "\"C:\\dir with space\\\\\""
        );
        assert_eq!(quote_arg("中文 prompt"), "\"中文 prompt\"");
    }

    #[test]
    fn batch_files_get_arguments_escaped_for_cmd() {
        let args = ["-i", "say \"hi\" & calc", "100%", r"a\", "--model=x"].map(OsStr::new);
        assert_eq!(
            batch_arguments(Path::new(r"C:\tools\copilot.cmd"), &args).unwrap(),
            r#"/e:ON /v:OFF /d /c ""C:\tools\copilot.cmd" -i "say ""hi"" & calc" "100%%cd:~,%" "a\\" "--model=x"""#
        );
        let line = batch_command_line(Path::new(r"C:\t\c.cmd"), &[]).unwrap();
        assert!(
            line.to_ascii_lowercase()
                .ends_with(r#"\system32\cmd.exe /e:on /v:off /d /c ""c:\t\c.cmd"""#),
            "{line}"
        );
        assert!(batch_arguments(Path::new(r"C:\t\c.cmd"), &[OsStr::new("two\nlines")]).is_err());
        assert!(batch_arguments(Path::new(r#"C:\t\"c.cmd"#), &[]).is_err());
    }

    /// Runs a real batch file: metacharacters must neither run commands nor expand variables.
    #[test]
    fn batch_arguments_reach_the_batch_file_literally() {
        let dir = std::env::temp_dir().join(format!("magicopilot-bat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("echo args.cmd");
        std::fs::write(&script, "@echo off\r\n>\"%~dp0out.txt\" echo(%*\r\n").unwrap();
        let injected = dir.join("injected.txt");
        let hostile = format!("x\" & echo pwned> \"{}\" & \"", injected.display());
        let args = [
            OsStr::new("a&b|c"),
            OsStr::new(&hostile),
            OsStr::new("%PATH%"),
            OsStr::new("^!"),
        ];
        let status = std::process::Command::new(system_dir().join("cmd.exe"))
            .raw_arg(batch_arguments(&script, &args).unwrap())
            .status()
            .unwrap();
        let out = std::fs::read_to_string(dir.join("out.txt")).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(status.success());
        assert!(
            !injected.exists(),
            "a quote in an argument ran a command: {out}"
        );
        assert!(
            out.contains("\"a&b|c\"") && out.contains("\"%PATH%\"") && out.contains("\"^!\""),
            "{out}"
        );
    }
}

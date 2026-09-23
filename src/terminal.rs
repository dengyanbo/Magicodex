use crossterm::{
    cursor::Show,
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
#[cfg(windows)]
use std::sync::atomic::{AtomicU32, Ordering};
use std::{
    io::{self, IsTerminal, Stdout},
    sync::atomic::AtomicBool,
};

#[cfg(windows)]
static ORIGINAL_INPUT_MODE: AtomicU32 = AtomicU32::new(u32::MAX);
#[cfg(windows)]
static ORIGINAL_CODE_PAGE: AtomicU32 = AtomicU32::new(0);
static ACTIVE: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
fn capture_input_state() -> io::Result<()> {
    use windows_sys::Win32::System::Console::{
        GetConsoleCP, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE,
    };
    unsafe {
        let mut mode = 0;
        if GetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), &mut mode) == 0 {
            return Err(io::Error::last_os_error());
        }
        ORIGINAL_INPUT_MODE.store(mode, Ordering::SeqCst);
        ORIGINAL_CODE_PAGE.store(GetConsoleCP(), Ordering::SeqCst);
    }
    Ok(())
}

#[cfg(windows)]
fn enable_vt_input() -> io::Result<()> {
    use windows_sys::Win32::System::Console::{
        ENABLE_VIRTUAL_TERMINAL_INPUT, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE,
        SetConsoleCP, SetConsoleMode,
    };
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        let mut mode = 0;
        if GetConsoleMode(handle, &mut mode) == 0 {
            return Err(io::Error::last_os_error());
        }
        if SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_INPUT) == 0
            || SetConsoleCP(65001) == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(windows)]
fn restore_vt_input() -> io::Result<()> {
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_INPUT_HANDLE, SetConsoleCP, SetConsoleMode,
    };
    let mode = ORIGINAL_INPUT_MODE.swap(u32::MAX, Ordering::SeqCst);
    let cp = ORIGINAL_CODE_PAGE.swap(0, Ordering::SeqCst);
    unsafe {
        if mode != u32::MAX && SetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), mode) == 0 {
            return Err(io::Error::last_os_error());
        }
        if cp != 0 && SetConsoleCP(cp) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub struct TerminalGuard {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}

pub fn restore() -> io::Result<()> {
    if !ACTIVE.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Ok(());
    }
    let raw = disable_raw_mode();
    #[cfg(windows)]
    let vt = restore_vt_input();
    let screen = execute!(
        io::stdout(),
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    );
    #[cfg(windows)]
    return raw.and(vt).and(screen);
    #[cfg(not(windows))]
    raw.and(screen)
}

impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(io::Error::other(
                "交互模式需要真实终端。请在 Windows Terminal 运行，或使用 --demo --snapshot。",
            ));
        }
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Err(error) = restore() {
                eprintln!("终端恢复失败：{error}");
            }
            previous(info);
        }));
        #[cfg(windows)]
        capture_input_state()?;
        ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);
        let result = (|| {
            enable_raw_mode()?;
            #[cfg(windows)]
            enable_vt_input()?;
            execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
            Terminal::new(CrosstermBackend::new(io::stdout()))
        })();
        match result {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                if let Err(cleanup) = restore() {
                    eprintln!("终端恢复失败：{cleanup}");
                }
                Err(error)
            }
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if let Err(error) = restore() {
            eprintln!("终端恢复失败：{error}");
        }
    }
}

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn example(name: &str) -> String {
    root()
        .join("examples")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bloom-it-022-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, content).expect("write file");
}

fn norm(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn no_toolchain(out: &std::process::Output) -> bool {
    out.status.code() == Some(3)
}

fn run_bloom(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_bloom"))
        .args(args)
        .output()
        .expect("run bloom")
}

mod win32 {
    pub type Hwnd = isize;

    #[repr(C)]
    pub struct Rect {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }

    #[repr(C)]
    pub struct GuiThreadInfo {
        pub cb_size: i32,
        pub flags: i32,
        pub hwnd_active: isize,
        pub hwnd_focus: isize,
        pub hwnd_capture: isize,
        pub hwnd_menu_owner: isize,
        pub hwnd_move_size: isize,
        pub hwnd_caret: isize,
        pub rc_caret: Rect,
    }

    pub const WM_CLOSE: u32 = 0x0010;
    pub const WM_CHAR: u32 = 0x0102;
    pub const WM_COMMAND: u32 = 0x0111;
    pub const WM_KEYDOWN: u32 = 0x0100;
    pub const WM_KEYUP: u32 = 0x0101;
    pub const BM_CLICK: u32 = 0x00F5;
    pub const VK_TAB: u8 = 0x09;
    pub const VK_SHIFT: u8 = 0x10;
    pub const MF_BYCOMMAND: u32 = 0;
    pub const MF_GRAYED: u32 = 1;

    #[link(name = "user32")]
    extern "system" {
        pub fn EnumWindows(cb: extern "system" fn(Hwnd, isize) -> i32, lparam: isize) -> i32;
        pub fn EnumChildWindows(
            parent: Hwnd,
            cb: extern "system" fn(Hwnd, isize) -> i32,
            lparam: isize,
        ) -> i32;
        pub fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
        pub fn GetWindowTextLengthW(hwnd: Hwnd) -> i32;
        pub fn GetWindowTextW(hwnd: Hwnd, buf: *mut u16, max: i32) -> i32;
        pub fn GetClassNameW(hwnd: Hwnd, buf: *mut u16, max: i32) -> i32;
        pub fn GetDlgItem(hwnd: Hwnd, id: i32) -> Hwnd;
        pub fn GetDlgCtrlID(hwnd: Hwnd) -> i32;
        pub fn SendMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize;
        pub fn PostMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> i32;
        pub fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
        pub fn GetGUIThreadInfo(tid: u32, info: *mut GuiThreadInfo) -> i32;
        pub fn keybd_event(vk: u8, scan: u8, flags: u32, extra: usize);
        pub fn SetWindowPos(
            hwnd: Hwnd,
            after: Hwnd,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            flags: u32,
        ) -> i32;
        pub fn GetMenu(hwnd: Hwnd) -> isize;
        pub fn GetMenuState(menu: isize, id: u32, flags: u32) -> u32;
    }
}

fn desktop_build(dir: &Path, name: &str, source: &str, console: bool) -> Option<PathBuf> {
    let src = dir.join(format!("{name}.blm"));
    write_file(&src, source);
    let exe = dir.join(format!("{name}.exe"));
    let mut args = vec![
        "build".to_string(),
        src.to_string_lossy().to_string(),
        "-o".to_string(),
        exe.to_string_lossy().to_string(),
    ];
    if console {
        args.push("--console".to_string());
    }
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let build = run_bloom(&refs);
    if no_toolchain(&build) {
        eprintln!("skipping: no C toolchain available");
        return None;
    }
    assert_eq!(
        build.status.code(),
        Some(0),
        "{}\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    Some(exe)
}

struct DesktopRun {
    child: std::process::Child,
    reader: std::io::BufReader<std::process::ChildStdout>,
}

impl DesktopRun {
    fn read_line(&mut self) -> String {
        use std::io::BufRead;
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).expect("read line");
        assert!(n > 0, "desktop program exited before printing a line");
        line.trim_end_matches(['\r', '\n']).to_string()
    }

    fn finish(mut self) -> (i32, String, String) {
        use std::io::Read;
        let status = self.child.wait().expect("wait desktop child");
        let mut rest = String::new();
        self.reader.read_to_string(&mut rest).expect("read rest");
        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .expect("stderr")
            .read_to_string(&mut stderr)
            .expect("stderr read");
        (status.code().unwrap_or(-1), norm(&rest), norm(&stderr))
    }
}

fn desktop_spawn(exe: &Path) -> DesktopRun {
    let mut child = Command::new(exe)
        .env("BLOOM_MEM_REPORT", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn desktop program");
    let reader = std::io::BufReader::new(child.stdout.take().expect("stdout"));
    DesktopRun { child, reader }
}

fn desktop_read_until(run: &mut DesktopRun, needle: &str) -> String {
    let mut seen = String::new();
    for _ in 0..200 {
        let line = run.read_line();
        seen.push_str(&line);
        seen.push('\n');
        if line.contains(needle) {
            return seen;
        }
    }
    panic!("did not see `{needle}` in: {seen}");
}

struct FindState {
    pid: u32,
    class: String,
    title: String,
    found: isize,
}

extern "system" fn find_cb(hwnd: isize, lp: isize) -> i32 {
    let state = unsafe { &mut *(lp as *mut FindState) };
    let mut pid: u32 = 0;
    unsafe {
        win32::GetWindowThreadProcessId(hwnd, &mut pid);
    }
    if pid != state.pid {
        return 1;
    }
    let class = unsafe { class_name(hwnd) };
    if class != state.class {
        return 1;
    }
    if !state.title.is_empty() {
        let title = unsafe { window_title(hwnd) };
        if title != state.title {
            return 1;
        }
    }
    state.found = hwnd;
    0
}

unsafe fn window_title(hwnd: isize) -> String {
    let len = win32::GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; (len + 1) as usize];
    let got = win32::GetWindowTextW(hwnd, buf.as_mut_ptr(), len + 1);
    if got <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..got as usize])
}

unsafe fn class_name(hwnd: isize) -> String {
    let mut buf = [0u16; 128];
    let got = win32::GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if got <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..got as usize])
}

fn find_top(pid: u32, class: &str, title: &str) -> Option<isize> {
    let mut state = FindState {
        pid,
        class: class.to_string(),
        title: title.to_string(),
        found: 0,
    };
    unsafe {
        win32::EnumWindows(find_cb, &mut state as *mut FindState as isize);
    }
    if state.found == 0 {
        None
    } else {
        Some(state.found)
    }
}

fn wait_top(pid: u32, class: &str, title: &str) -> isize {
    for _ in 0..100 {
        if let Some(hwnd) = find_top(pid, class, title) {
            return hwnd;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("top-level window `{title}` ({class}) not found");
}

fn wait_window(pid: u32, prefix: &str) -> isize {
    for _ in 0..100 {
        if let Some(hwnd) = find_top(pid, "BloomDesktopWindow", "") {
            let title = unsafe { window_title(hwnd) };
            if title.starts_with(prefix) {
                return hwnd;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("desktop window `{prefix}` not found");
}

struct ChildState {
    class: String,
    id: i32,
    text: Option<String>,
    found: isize,
}

extern "system" fn child_cb(hwnd: isize, lp: isize) -> i32 {
    let state = unsafe { &mut *(lp as *mut ChildState) };
    if unsafe { class_name(hwnd) } != state.class {
        return 1;
    }
    if state.id >= 0 {
        let id = unsafe { win32::GetDlgCtrlID(hwnd) };
        if id != state.id {
            return 1;
        }
    }
    if let Some(text) = &state.text {
        if &unsafe { window_title(hwnd) } != text {
            return 1;
        }
    }
    state.found = hwnd;
    0
}

fn find_child(parent: isize, class: &str, id: i32) -> Option<isize> {
    let mut state = ChildState {
        class: class.to_string(),
        id,
        text: None,
        found: 0,
    };
    unsafe {
        win32::EnumChildWindows(parent, child_cb, &mut state as *mut ChildState as isize);
    }
    if state.found == 0 {
        None
    } else {
        Some(state.found)
    }
}

fn find_child_text(parent: isize, class: &str, text: &str) -> Option<isize> {
    let mut state = ChildState {
        class: class.to_string(),
        id: -1,
        text: Some(text.to_string()),
        found: 0,
    };
    unsafe {
        win32::EnumChildWindows(parent, child_cb, &mut state as *mut ChildState as isize);
    }
    if state.found == 0 {
        None
    } else {
        Some(state.found)
    }
}

fn click(hwnd: isize) {
    unsafe {
        win32::SendMessageW(hwnd, win32::BM_CLICK, 0, 0);
    }
}

fn close(hwnd: isize) {
    unsafe {
        win32::PostMessageW(hwnd, win32::WM_CLOSE, 0, 0);
    }
}

fn set_size(hwnd: isize, width: i32, height: i32) {
    unsafe {
        win32::SetWindowPos(hwnd, 0, 0, 0, width, height, 0x0002 | 0x0004 | 0x0010);
    }
    std::thread::sleep(std::time::Duration::from_millis(120));
}

fn thread_focus(hwnd: isize) -> isize {
    let tid = unsafe { win32::GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
    let mut info = win32::GuiThreadInfo {
        cb_size: std::mem::size_of::<win32::GuiThreadInfo>() as i32,
        flags: 0,
        hwnd_active: 0,
        hwnd_focus: 0,
        hwnd_capture: 0,
        hwnd_menu_owner: 0,
        hwnd_move_size: 0,
        hwnd_caret: 0,
        rc_caret: win32::Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
    };
    unsafe {
        win32::GetGUIThreadInfo(tid, &mut info);
    }
    info.hwnd_focus
}

fn tab(hwnd: isize, shift: bool) {
    if shift {
        unsafe {
            win32::keybd_event(win32::VK_SHIFT, 0, 0, 0);
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    let focus = thread_focus(hwnd);
    let target = if focus != 0 { focus } else { hwnd };
    unsafe {
        win32::PostMessageW(target, win32::WM_KEYDOWN, win32::VK_TAB as usize, 0);
        win32::PostMessageW(target, win32::WM_KEYUP, win32::VK_TAB as usize, 0);
    }
    std::thread::sleep(std::time::Duration::from_millis(80));
    if shift {
        unsafe {
            win32::keybd_event(win32::VK_SHIFT, 0, 2, 0);
        }
    }
}

fn parse_xywh(line: &str) -> (i64, i64, i64, i64) {
    let value = line.split_once('=').expect("value").1.trim();
    let (x, rest) = value.split_once(',').expect("x");
    let (y, rest) = rest.split_once(',').expect("y");
    let (w, h) = rest.split_once('x').expect("size");
    (
        x.parse().expect("x"),
        y.parse().expect("y"),
        w.parse().expect("w"),
        h.parse().expect("h"),
    )
}

#[test]
fn desktop_layout_natively() {
    let dir = temp_dir("layout");
    let source = r#"use std::desktop;

fn bounds(win: desktop::Window, label: string, control: int) -> void {
    match desktop::control_bounds(win, control) {
        Result::Ok(b) => println(label + "=" + str(b.x) + "," + str(b.y) + "," + str(b.width) + "x" + str(b.height)),
        Result::Err(e) => println(label + "-err: " + e)
    };
}

fn report(win: desktop::Window) -> void {
    match desktop::window_client_size(win) {
        Result::Ok(s) => report_size(win, s),
        Result::Err(e) => println("client-err: " + e)
    };
}

fn report_size(win: desktop::Window, size: desktop::Size) -> void {
    println("client=" + str(size.width) + "x" + str(size.height));
    bounds(win, "a", 101);
    bounds(win, "b", 102);
    bounds(win, "c", 103);
}

fn on_event(win: desktop::Window, ev: desktop::Event) -> bool {
    if ev.kind == "click" {
        report(win);
    } else if ev.kind == "close" {
        println("Fenster geschlossen");
    }
    return true;
}

fn step(win: desktop::Window) -> bool {
    return match desktop::next_event(win) {
        Result::Ok(ev) => on_event(win, ev),
        Result::Err(e) => fail(e)
    };
}

fn fail(e: string) -> bool {
    println("FEHLER: " + e);
    return false;
}

fn run() -> Result<int, string> {
    let win = desktop::window_create("Bloom IT Layout2", 600, 400)?;
    let root = desktop::layout_create(win, "column")?;
    desktop::layout_set_padding(win, root, 10)?;
    desktop::layout_set_spacing(win, root, 10)?;
    let head = desktop::label_create(win, "Kopf", 0, 0, 10, 10)?;
    desktop::layout_add_control(win, root, head, desktop::LayoutItem {
        flex: 0, width: 0, height: 30, align: ""
    })?;
    let row = desktop::layout_create(win, "row")?;
    let a = desktop::button_create(win, "A", 0, 0, 10, 10)?;
    let b = desktop::button_create(win, "B", 0, 0, 10, 10)?;
    let c = desktop::button_create(win, "C", 0, 0, 10, 10)?;
    desktop::layout_add_control(win, row, a, desktop::LayoutItem {
        flex: 1, width: 0, height: 40, align: ""
    })?;
    desktop::layout_add_control(win, row, b, desktop::LayoutItem {
        flex: 2, width: 0, height: 40, align: ""
    })?;
    desktop::layout_add_control(win, row, c, desktop::LayoutItem {
        flex: 0, width: 100, height: 40, align: ""
    })?;
    desktop::layout_add_layout(win, root, row, desktop::LayoutItem {
        flex: 1, width: 0, height: 0, align: ""
    })?;
    let status = desktop::label_create(win, "Status", 0, 0, 10, 10)?;
    desktop::layout_add_control(win, root, status, desktop::LayoutItem {
        flex: 0, width: 0, height: 20, align: ""
    })?;
    desktop::layout_attach(win, root)?;
    desktop::window_show(win)?;
    while step(win) {
        if !desktop::window_alive(win) {
            break;
        }
    }
    return Result::Ok(0);
}

fn main() -> int {
    return match run() {
        Result::Ok(code) => code,
        Result::Err(e) => fehler(e)
    };
}

fn fehler(e: string) -> int {
    println("FEHLER: " + e);
    return 1;
}
"#;
    let exe = match desktop_build(&dir, "layout2", source, true) {
        Some(p) => p,
        None => return,
    };
    let mut run = desktop_spawn(&exe);
    let hwnd = wait_window(run.child.id(), "Bloom IT Layout2");
    let a = unsafe { win32::GetDlgItem(hwnd, 101) };
    assert!(a != 0);
    click(a);
    let first = desktop_read_until(&mut run, "c=");
    let mut client = (0i64, 0i64);
    let mut bounds = [(0i64, 0i64, 0i64, 0i64); 3];
    for line in first.lines() {
        if let Some(value) = line.strip_prefix("client=") {
            let (w, h) = value.split_once('x').expect("client");
            client = (w.parse().unwrap(), h.parse().unwrap());
        } else if line.starts_with("a=") {
            bounds[0] = parse_xywh(line);
        } else if line.starts_with("b=") {
            bounds[1] = parse_xywh(line);
        } else if line.starts_with("c=") {
            bounds[2] = parse_xywh(line);
        }
    }
    assert_eq!(client, (600, 400), "{first}");
    assert_eq!(bounds[0].0, 10, "{first}");
    assert_eq!(bounds[0].1, 50, "{first}");
    assert_eq!(bounds[0].3, 40, "{first}");
    assert_eq!(bounds[2].2, 100, "{first}");
    assert_eq!(bounds[2].3, 40, "{first}");
    assert_eq!(bounds[1].0, bounds[0].0 + bounds[0].2, "{first}");
    assert_eq!(bounds[2].0, bounds[1].0 + bounds[1].2, "{first}");
    assert_eq!(bounds[2].0 + bounds[2].2, 590, "{first}");
    assert!(bounds[1].2 > bounds[0].2, "{first}");

    set_size(hwnd, 780, 480);
    click(a);
    let second = desktop_read_until(&mut run, "c=");
    let mut client2 = (0i64, 0i64);
    let mut bounds2 = [(0i64, 0i64, 0i64, 0i64); 3];
    for line in second.lines() {
        if let Some(value) = line.strip_prefix("client=") {
            let (w, h) = value.split_once('x').expect("client");
            client2 = (w.parse().unwrap(), h.parse().unwrap());
        } else if line.starts_with("a=") {
            bounds2[0] = parse_xywh(line);
        } else if line.starts_with("b=") {
            bounds2[1] = parse_xywh(line);
        } else if line.starts_with("c=") {
            bounds2[2] = parse_xywh(line);
        }
    }
    assert!(client2.0 > client.0, "{second}");
    assert!(client2.1 > client.1, "{second}");
    assert_eq!(bounds2[2].2, 100, "{second}");
    assert_eq!(bounds2[2].0 + bounds2[2].2, client2.0 - 10, "{second}");
    assert!(bounds2[0].2 > bounds[0].2, "{second}");
    close(hwnd);
    let (code, rest, stderr) = run.finish();
    assert_eq!(code, 0, "{rest}\n{stderr}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn desktop_controls_natively() {
    let dir = temp_dir("controls");
    let source = r#"use std::desktop;

fn on_event(ev: desktop::Event) -> bool {
    if ev.kind == "select" {
        println("select:" + str(ev.control) + ":" + ev.text);
    } else if ev.kind == "input" {
        println("input:" + str(ev.control) + ":" + str(len(ev.text)));
    } else if ev.kind == "close" {
        println("close");
    }
    return true;
}

fn step(win: desktop::Window) -> bool {
    return match desktop::next_event(win) {
        Result::Ok(ev) => on_event(ev),
        Result::Err(e) => fail(e)
    };
}

fn fail(e: string) -> bool {
    println("FEHLER: " + e);
    return false;
}

fn run() -> Result<int, string> {
    let win = desktop::window_create("Bloom IT Controls", 480, 360)?;
    let combo = desktop::combo_create(win, "Grüße", 0, 0, 10, 10)?;
    desktop::combo_add(win, combo, "日本語")?;
    desktop::combo_add(win, combo, "🚀")?;
    println("combo-count=" + str(desktop::combo_count(win, combo)?));
    println("combo-selected=" + str(desktop::combo_selected(win, combo)?));
    println("combo-text=" + desktop::combo_selected_text(win, combo)?);
    desktop::combo_set_selected(win, combo, 2)?;
    println("combo-text2=" + desktop::combo_selected_text(win, combo)?);
    match desktop::combo_set_selected(win, combo, 9) {
        Result::Ok(_) => println("combo-bad=accepted"),
        Result::Err(_) => println("combo-bad=rejected")
    };
    let list = desktop::list_create(win, 0, 0, 10, 10)?;
    desktop::list_add(win, list, "Alpha")?;
    desktop::list_add(win, list, "Beta")?;
    desktop::list_add(win, list, "Grüße 🚀")?;
    println("list-count=" + str(desktop::list_count(win, list)?));
    desktop::list_set_selected(win, list, 2)?;
    println("list-text=" + desktop::list_selected_text(win, list)?);
    desktop::list_remove(win, list, 0)?;
    println("list-count2=" + str(desktop::list_count(win, list)?));
    match desktop::list_remove(win, list, 7) {
        Result::Ok(_) => println("list-bad=accepted"),
        Result::Err(_) => println("list-bad=rejected")
    };
    let area = desktop::text_area_create(win, "Zeile 1\nZeile 2", 0, 0, 10, 10)?;
    let back = desktop::control_text(win, area)?;
    println("area-lines=" + str(len(split(back, "\n"))));
    desktop::control_set_text(win, area, "Neu\nZeile")?;
    let back2 = desktop::control_text(win, area)?;
    println("area-lines2=" + str(len(split(back2, "\n"))) + " len=" + str(len(back2)));
    println("cross=" + str(desktop::debug_cross_thread(win)?));
    println("READY");
    desktop::window_show(win)?;
    while step(win) {
        if !desktop::window_alive(win) {
            break;
        }
    }
    return Result::Ok(0);
}

fn main() -> int {
    return match run() {
        Result::Ok(code) => code,
        Result::Err(e) => fehler(e)
    };
}

fn fehler(e: string) -> int {
    println("FEHLER: " + e);
    return 1;
}
"#;
    let exe = match desktop_build(&dir, "controls", source, true) {
        Some(p) => p,
        None => return,
    };
    let mut run = desktop_spawn(&exe);
    let first = desktop_read_until(&mut run, "READY");
    for needle in [
        "combo-count=3",
        "combo-selected=0",
        "combo-text=Grüße",
        "combo-text2=🚀",
        "combo-bad=rejected",
        "list-count=3",
        "list-text=Grüße 🚀",
        "list-count2=2",
        "list-bad=rejected",
        "area-lines=2",
        "cross=true",
    ] {
        assert!(first.contains(needle), "missing `{needle}` in {first}");
    }
    let hwnd = wait_window(run.child.id(), "Bloom IT Controls");
    let combo = unsafe { win32::GetDlgItem(hwnd, 100) };
    let list = unsafe { win32::GetDlgItem(hwnd, 101) };
    let area = unsafe { win32::GetDlgItem(hwnd, 102) };
    assert!(combo != 0 && list != 0 && area != 0);
    unsafe {
        win32::SendMessageW(combo, 0x014E, 1, 0);
        win32::PostMessageW(
            hwnd,
            win32::WM_COMMAND,
            ((1u16 as usize) << 16) | 100,
            combo,
        );
    }
    let select1 = desktop_read_until(&mut run, "select:100:");
    assert!(select1.contains("select:100:日本語"), "{select1}");
    unsafe {
        win32::SendMessageW(list, 0x0186, 0, 0);
        win32::PostMessageW(hwnd, win32::WM_COMMAND, ((1u16 as usize) << 16) | 101, list);
    }
    let select2 = desktop_read_until(&mut run, "select:101:");
    assert!(select2.contains("select:101:Beta"), "{select2}");
    unsafe {
        win32::SendMessageW(area, win32::WM_CHAR, 'X' as usize, 0);
    }
    let input = desktop_read_until(&mut run, "input:102:");
    assert!(input.contains("input:102:10"), "{input}");
    close(hwnd);
    let (code, rest, stderr) = run.finish();
    assert_eq!(code, 0, "{rest}\n{stderr}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn desktop_event_queue_natively() {
    let dir = temp_dir("queue");
    let source = r#"use std::desktop;

fn main() -> int {
    return match run() {
        Result::Ok(code) => code,
        Result::Err(e) => fehler(e)
    };
}

var clicks: int = 0;

fn count_event(ev: desktop::Event) -> void {
    if ev.kind == "click" {
        clicks = clicks + 1;
    }
}

fn run() -> Result<int, string> {
    let win = desktop::window_create("Bloom IT Queue", 240, 160)?;
    println("dropped=" + str(desktop::debug_event_flood(win, 70)?));
    println("total-dropped=" + str(desktop::events_dropped(win)?));
    var read: int = 0;
    while read < 64 {
        match desktop::next_event(win) {
            Result::Ok(ev) => count_event(ev),
            Result::Err(e) => println("early: " + e)
        };
        read = read + 1;
    }
    println("clicks=" + str(clicks));
    desktop::window_close(win)?;
    match desktop::next_event(win) {
        Result::Ok(ev) => println("first-close=" + ev.kind),
        Result::Err(e) => println("first-close-err: " + e)
    };
    match desktop::next_event(win) {
        Result::Ok(ev) => println("unexpected=" + ev.kind),
        Result::Err(e) => println("after-close=" + e)
    };
    return Result::Ok(0);
}

fn fehler(e: string) -> int {
    println("FEHLER: " + e);
    return 1;
}
"#;
    let exe = match desktop_build(&dir, "queue", source, true) {
        Some(p) => p,
        None => return,
    };
    let out = Command::new(&exe)
        .env("BLOOM_MEM_REPORT", "1")
        .output()
        .expect("run queue program");
    let stdout = norm(&String::from_utf8_lossy(&out.stdout));
    let stderr = norm(&String::from_utf8_lossy(&out.stderr));
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{stderr}");
    for needle in [
        "dropped=6",
        "total-dropped=6",
        "clicks=64",
        "first-close=close",
        "after-close=next_event: window is closed",
    ] {
        assert!(stdout.contains(needle), "missing `{needle}` in {stdout}");
    }
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn desktop_message_box_natively() {
    let dir = temp_dir("msgbox");
    let source = r#"use std::desktop;

fn main() -> int {
    return match run() {
        Result::Ok(code) => code,
        Result::Err(e) => fehler(e)
    };
}

fn run() -> Result<int, string> {
    let win = desktop::window_create("Bloom IT MsgBox", 200, 120)?;
    println("READY");
    let ok = desktop::message_box("Bloom IT Box", "Bitte bestaetigen")?;
    println("msgbox=" + str(ok));
    desktop::window_close(win)?;
    return Result::Ok(0);
}

fn fehler(e: string) -> int {
    println("FEHLER: " + e);
    return 1;
}
"#;
    let exe = match desktop_build(&dir, "msgbox", source, true) {
        Some(p) => p,
        None => return,
    };
    let mut run = desktop_spawn(&exe);
    assert_eq!(run.read_line(), "READY");
    let dialog = wait_top(run.child.id(), "#32770", "Bloom IT Box");
    let ok = find_child_text(dialog, "Button", "OK")
        .or_else(|| find_child(dialog, "Button", 2))
        .expect("OK button");
    click(ok);
    let (code, rest, stderr) = run.finish();
    assert_eq!(code, 0, "{rest}\n{stderr}");
    assert!(rest.contains("msgbox=true"), "{rest}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn desktop_file_dialog_natively() {
    let dir = temp_dir("filedialog");
    let open_src = r#"use std::desktop;

fn run() -> Result<int, string> {
    let win = desktop::window_create("Bloom IT OpenOne", 200, 120)?;
    println("READY");
    match desktop::open_file_dialog(win, "Bloom IT Open3", "Text (*.txt)|*.txt|Alle (*.*)|*.*|") {
        Result::Ok(path) => match path {
            Option::Some(p) => println("path=" + p),
            Option::None => println("Oeffnen abgebrochen.")
        },
        Result::Err(e) => println("open-error: " + e)
    };
    desktop::window_close(win)?;
    return Result::Ok(0);
}

fn main() -> int {
    return match run() {
        Result::Ok(code) => code,
        Result::Err(e) => fehler(e)
    };
}

fn fehler(e: string) -> int {
    println("FEHLER: " + e);
    return 1;
}
"#;
    let save_src = r#"use std::desktop;

fn run() -> Result<int, string> {
    let win = desktop::window_create("Bloom IT SaveOne", 200, 120)?;
    println("READY");
    match desktop::save_file_dialog(win, "Bloom IT Save3", "Text (*.txt)|*.txt|Alle (*.*)|*.*|", "bloom_demo.txt") {
        Result::Ok(path) => match path {
            Option::Some(p) => println("path=" + p),
            Option::None => println("Speichern abgebrochen.")
        },
        Result::Err(e) => println("save-error: " + e)
    };
    desktop::window_close(win)?;
    return Result::Ok(0);
}

fn main() -> int {
    return match run() {
        Result::Ok(code) => code,
        Result::Err(e) => fehler(e)
    };
}

fn fehler(e: string) -> int {
    println("FEHLER: " + e);
    return 1;
}
"#;
    let open_exe = match desktop_build(&dir, "open_one", open_src, true) {
        Some(p) => p,
        None => return,
    };
    let save_exe = match desktop_build(&dir, "save_one", save_src, true) {
        Some(p) => p,
        None => return,
    };

    let mut run = desktop_spawn(&open_exe);
    assert_eq!(run.read_line(), "READY");
    let dialog = wait_top(run.child.id(), "#32770", "Bloom IT Open3");
    let cancel = find_child(dialog, "Button", 2).expect("cancel");
    click(cancel);
    let cancelled = desktop_read_until(&mut run, "Oeffnen abgebrochen.");
    assert!(cancelled.contains("Oeffnen abgebrochen."));
    let (code, rest, stderr) = run.finish();
    assert_eq!(code, 0, "{rest}\n{stderr}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");

    let mut run = desktop_spawn(&save_exe);
    assert_eq!(run.read_line(), "READY");
    let dialog = wait_top(run.child.id(), "#32770", "Bloom IT Save3");
    let cancel = find_child(dialog, "Button", 2).expect("cancel save");
    click(cancel);
    let cancelled = desktop_read_until(&mut run, "Speichern abgebrochen.");
    assert!(cancelled.contains("Speichern abgebrochen."));
    let (code, rest, stderr) = run.finish();
    assert_eq!(code, 0, "{rest}\n{stderr}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}
#[test]
fn desktop_menu_natively() {
    let dir = temp_dir("menu");
    let source = std::fs::read_to_string(example("desktop_menu.blm")).expect("example");
    let exe = match desktop_build(&dir, "menu", &source, true) {
        Some(p) => p,
        None => return,
    };
    let mut run = desktop_spawn(&exe);
    let hwnd = wait_window(run.child.id(), "Bloom Desktop: Menue");
    let menu = unsafe { win32::GetMenu(hwnd) };
    assert!(menu != 0, "window must have a menu");
    let state = unsafe { win32::GetMenuState(menu, 1004, win32::MF_BYCOMMAND) };
    assert_eq!(
        state & win32::MF_GRAYED,
        win32::MF_GRAYED,
        "Bearbeiten/Deaktiviert must be disabled"
    );
    unsafe {
        win32::PostMessageW(hwnd, win32::WM_COMMAND, 1001, 0);
    }
    let first = desktop_read_until(&mut run, "Menue: Neu");
    assert!(first.contains("Menue: Neu"), "{first}");
    unsafe {
        win32::PostMessageW(hwnd, win32::WM_COMMAND, 1002, 0);
    }
    let done = desktop_read_until(&mut run, "Menue-Ende");
    let (code, rest, stderr) = run.finish();
    let mut all = done;
    all.push_str(&rest);
    assert_eq!(code, 0, "{all}\n{stderr}");
    assert!(all.contains("Menue: Ende"), "{all}");
    assert!(all.contains("Menue-Ende"), "{all}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn desktop_keyboard_natively() {
    let dir = temp_dir("keyboard");
    let source = std::fs::read_to_string(example("desktop_keyboard.blm")).expect("example");
    let exe = match desktop_build(&dir, "keyboard", &source, true) {
        Some(p) => p,
        None => return,
    };
    let mut run = desktop_spawn(&exe);
    assert_eq!(run.read_line(), "READY focus=101");
    let hwnd = wait_window(run.child.id(), "Bloom Desktop: Tastatur");
    let a = unsafe { win32::GetDlgItem(hwnd, 102) };
    let b = unsafe { win32::GetDlgItem(hwnd, 103) };
    assert!(a != 0 && b != 0);

    tab(hwnd, false);
    click(a);
    let first = desktop_read_until(&mut run, "click:102");
    assert!(first.contains("focus=102"), "{first}");

    tab(hwnd, false);
    click(b);
    let second = desktop_read_until(&mut run, "click:103");
    assert!(second.contains("focus=103"), "{second}");

    tab(hwnd, true);
    click(a);
    let third = desktop_read_until(&mut run, "click:102");
    assert!(third.contains("focus=102"), "{third}");

    tab(hwnd, false);
    tab(hwnd, false);
    click(b);
    let wrapped = desktop_read_until(&mut run, "click:103");
    assert!(
        wrapped.contains("focus=103"),
        "focus must stay on the last tab stop, not disabled/hidden controls: {wrapped}"
    );
    assert!(!wrapped.contains("focus=104"), "{wrapped}");
    assert!(!wrapped.contains("focus=105"), "{wrapped}");

    close(hwnd);
    let (code, rest, stderr) = run.finish();
    assert_eq!(code, 0, "{rest}\n{stderr}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn desktop_dpi_math_natively() {
    let dir = temp_dir("dpi");
    let source = std::fs::read_to_string(example("desktop_dpi.blm")).expect("example");
    let exe = match desktop_build(&dir, "dpi", &source, true) {
        Some(p) => p,
        None => return,
    };
    let mut run = desktop_spawn(&exe);
    let info = run.read_line();
    assert_eq!(run.read_line(), "dpi144=450");
    let mut dpi: i64 = 96;
    let mut client = (0i64, 0i64);
    for part in info.split(", ") {
        if let Some(value) = part.strip_prefix("DPI ") {
            dpi = value.parse().expect("dpi");
        } else if let Some(value) = part.strip_prefix("Client ") {
            let (w, h) = value.split_once('x').expect("client");
            client = (w.parse().unwrap(), h.parse().unwrap());
        }
    }
    assert!((96..=960).contains(&dpi), "{info}");
    let scaled = (300 * dpi + 48) / 96;
    assert!(info.contains(&format!("300 -> {scaled} Pixel")), "{info}");
    let hwnd = wait_window(run.child.id(), "Bloom Desktop: DPI");
    let mut rect = win32::Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let ok = unsafe { win32::GetClientRect(hwnd, &mut rect) };
    assert!(ok != 0);
    let pw = (rect.right - rect.left) as i64;
    let ph = (rect.bottom - rect.top) as i64;
    let ew = (client.0 * dpi + 48) / 96;
    let eh = (client.1 * dpi + 48) / 96;
    assert!((pw - ew).abs() <= 2, "physical {pw} vs {ew} at {dpi} dpi");
    assert!((ph - eh).abs() <= 2, "physical {ph} vs {eh} at {dpi} dpi");
    close(hwnd);
    let (code, rest, stderr) = run.finish();
    assert_eq!(code, 0, "{rest}\n{stderr}");
    assert!(stderr.contains("live allocations: 0"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn desktop_examples_022_natively() {
    let dir = temp_dir("examples022");
    let examples = [
        "desktop_layout_stack.blm",
        "desktop_resize.blm",
        "desktop_combo_box.blm",
        "desktop_text_area.blm",
        "desktop_list_box.blm",
        "desktop_menu.blm",
        "desktop_file_dialog.blm",
        "desktop_keyboard.blm",
        "desktop_dpi.blm",
        "desktop_gallery.blm",
    ];
    for name in examples {
        let stem = name.trim_end_matches(".blm");
        let source = std::fs::read_to_string(example(name)).expect("read example");
        let exe = match desktop_build(&dir, stem, &source, true) {
            Some(p) => p,
            None => return,
        };
        let run = desktop_spawn(&exe);
        let hwnd = wait_window(run.child.id(), "Bloom Desktop");
        close(hwnd);
        let (code, rest, stderr) = run.finish();
        assert_eq!(code, 0, "{name}: {rest}\n{stderr}");
        assert!(!rest.contains("FEHLER"), "{name}: {rest}");
        assert!(stderr.contains("live allocations: 0"), "{name}: {stderr}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

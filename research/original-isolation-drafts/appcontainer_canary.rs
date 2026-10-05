//! Authored native process probe. Never launches or writes original game data.
use std::{ffi::c_void, fs::OpenOptions, io::{self, Read, Write}, ptr};

#[repr(C)]
struct Guid { a: u32, b: u16, c: u16, d: [u8; 8] }
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn LocalFree(handle: *mut c_void) -> *mut c_void;
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
    fn GetTokenInformation(token: *mut c_void, kind: u32, buffer: *mut c_void, bytes: u32, returned: *mut u32) -> i32;
    fn ConvertSidToStringSidW(sid: *mut c_void, output: *mut *mut u16) -> i32;
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn SHGetKnownFolderPath(folder: *const Guid, flags: u32, token: *mut c_void, output: *mut *mut u16) -> i32;
}
#[link(name = "ole32")]
unsafe extern "system" { fn CoTaskMemFree(memory: *mut c_void); }

fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""), '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"), '\r' => out.push_str("\\r"), '\t' => out.push_str("\\t"),
            c if c <= '\u{1f}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"'); out
}
unsafe fn wide(pointer: *const u16) -> io::Result<String> {
    if pointer.is_null() { return Err(io::Error::other("null API string")); }
    let mut words = Vec::new();
    for at in 0..32768 {
        // The API owns a null-terminated string; the fixed ceiling bounds retention.
        let word = unsafe { *pointer.add(at) };
        if word == 0 { return String::from_utf16(&words).map_err(io::Error::other); }
        words.push(word);
    }
    Err(io::Error::other("API string exceeds probe bound"))
}
fn token_identity() -> io::Result<(u32, String)> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), 8, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut container = 0u32;
        let mut returned = 0;
        if unsafe { GetTokenInformation(token, 29, (&mut container as *mut u32).cast(), 4, &mut returned) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if returned != 4 || container != 1 { return Err(io::Error::other("native child is not an AppContainer")); }
        let mut storage = [0u64; 512];
        if unsafe { GetTokenInformation(token, 31, storage.as_mut_ptr().cast(), 4096, &mut returned) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if returned < std::mem::size_of::<usize>() as u32 || returned > 4096 {
            return Err(io::Error::other("invalid token SID extent"));
        }
        let sid = unsafe { *storage.as_ptr().cast::<*mut c_void>() };
        let mut text = ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 { return Err(io::Error::last_os_error()); }
        let value = unsafe { wide(text) };
        unsafe { LocalFree(text.cast()); }
        Ok((container, value?))
    })();
    unsafe { CloseHandle(token); }
    result
}
fn folder(folder: &Guid) -> String {
    let mut output = ptr::null_mut();
    let result = unsafe { SHGetKnownFolderPath(folder, 0, ptr::null_mut(), &mut output) };
    let path = if result >= 0 { unsafe { wide(output) }.ok() } else { None };
    if !output.is_null() { unsafe { CoTaskMemFree(output.cast()); } }
    format!("{{\"hresult\":{},\"path\":{}}}", result as u32, path.as_deref().map(quote).unwrap_or_else(|| "null".into()))
}
fn probe() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 5 || !matches!(args[4].to_str(), Some("normal" | "linger")) {
        return Err(io::Error::other("expected private input, private fresh output, protected canary, mode"));
    }
    let (is_container, sid) = token_identity()?;
    let mut private = Vec::new();
    std::fs::File::open(&args[1])?.take(257).read_to_end(&mut private)?;
    let read_ok = private == b"rust-fallout-private-canary-v1\n";
    if !read_ok { return Err(io::Error::other("private input differs")); }
    let mut writable = OpenOptions::new().write(true).create_new(true).open(&args[2])?;
    writable.write_all(&private)?; writable.sync_all()?; drop(writable);
    // Request write access without creating, truncating or writing the protected file.
    let protected_error = match OpenOptions::new().write(true).open(&args[3]) {
        Ok(file) => { drop(file); 0 },
        Err(error) => error.raw_os_error().unwrap_or(-1),
    };
    let documents = Guid { a:0xfdd39ad0,b:0x238f,c:0x46af,d:[0xad,0xb4,0x6c,0x85,0x48,0x03,0x69,0xc7] };
    let local = Guid { a:0xf1b32785,b:0x6fba,c:0x4fcf,d:[0x9d,0x55,0x7b,0x8e,0x7f,0x15,0x70,0x91] };
    println!("{{\"schema_version\":1,\"pid\":{},\"pointer_bits\":{},\"is_appcontainer\":{},\"appcontainer_sid\":{},\"private_read\":{},\"private_write\":true,\"protected_write_open_error\":{},\"documents\":{},\"local_appdata\":{},\"environment_local_appdata\":{},\"environment_temp\":{},\"original_launched\":false}}",
        std::process::id(),usize::BITS,is_container,quote(&sid),read_ok,protected_error,folder(&documents),folder(&local),
        quote(&std::env::var("LOCALAPPDATA").unwrap_or_default()),quote(&std::env::var("TEMP").unwrap_or_default()));
    io::stdout().flush()?;
    if protected_error != 5 { return Err(io::Error::other("protected write refusal is not access denied")); }
    if args[4] == "linger" { std::thread::sleep(std::time::Duration::from_secs(60)); }
    Ok(())
}
fn main() {
    if let Err(error) = probe() { eprintln!("authored AppContainer canary failed: {error}"); std::process::exit(2); }
}

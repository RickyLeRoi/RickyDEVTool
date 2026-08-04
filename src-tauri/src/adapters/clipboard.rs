use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::exec;

pub enum ClipRead {
    Text(String),
    Files(Vec<PathBuf>),
    Image {
        png_path: PathBuf,
        mime: String,
        width: u32,
        height: u32,
    },
}

// 20260704 RG i path dinamici passano da argv (macOS) o env (Windows), mai interpolati
// nel testo dello script: sarebbe injection nella shell dell'interprete.
pub fn read_text() -> Option<String> {
    let mut cmd = read_command()?;
    let output = cmd.stderr(Stdio::null()).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

pub fn write_text(text: &str) -> Result<(), String> {
    let mut cmd = write_command().ok_or("clipboard non supportata su questo sistema")?;
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("avvio comando clipboard fallito: {e}"))?;
    {
        let mut stdin = child.stdin.take().ok_or("stdin non disponibile")?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("scrittura clipboard fallita: {e}"))?;
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("comando clipboard terminato con errore".to_string())
    }
}

pub fn supported() -> bool {
    read_command().is_some()
}

pub fn read(last: i64) -> (i64, Option<ClipRead>) {
    #[cfg(target_os = "macos")]
    {
        read_macos(last)
    }
    #[cfg(target_os = "windows")]
    {
        read_windows(last)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = last;
        (last, None)
    }
}

pub fn write_image(png_path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        run_ok(
            exec::sync_cmd("osascript")
                .args(["-l", "JavaScript", "-e", MAC_WRITE_IMG_JS, "--"])
                .arg(png_path),
            "scrittura immagine",
        )
    }
    #[cfg(target_os = "windows")]
    {
        run_ok(
            exec::sync_cmd("powershell")
                .args(["-Sta", "-NoProfile", "-Command", WIN_WRITE_IMG_PS])
                .env("RDT_CLIPPATH", png_path),
            "scrittura immagine",
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = png_path;
        Err("immagini non supportate su questo sistema".into())
    }
}

pub fn write_files(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("nessun file da copiare".into());
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = exec::sync_cmd("osascript");
        cmd.args(["-l", "JavaScript", "-e", MAC_WRITE_FILES_JS, "--"]);
        for p in paths {
            cmd.arg(p);
        }
        run_ok(&mut cmd, "copia file")
    }
    #[cfg(target_os = "windows")]
    {
        let joined = paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        run_ok(
            exec::sync_cmd("powershell")
                .args(["-Sta", "-NoProfile", "-Command", WIN_WRITE_FILES_PS])
                .env("RDT_CLIPPATHS", joined),
            "copia file",
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("copia file non supportata su questo sistema".into())
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn run_ok(cmd: &mut Command, what: &str) -> Result<(), String> {
    let out = cmd
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("{what} fallita: {e}"))?;
    if out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "ok" {
        Ok(())
    } else {
        Err(format!("{what} non riuscita"))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn temp_png_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("rdt-clip-read-{nanos}.png"))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn json_i64(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn json_u32(v: &Value) -> Option<u32> {
    v.as_u64()
        .map(|n| n as u32)
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn parse_read(stdout: &[u8], fallback_png: &Path, last: i64) -> (i64, Option<ClipRead>) {
    let Ok(json) = serde_json::from_slice::<Value>(stdout) else {
        return (last, None);
    };
    let change = json_i64(&json["change"]).unwrap_or(last);
    if json["unchanged"].as_bool() == Some(true) {
        return (change, None);
    }
    match json["kind"].as_str() {
        Some("files") => {
            let paths: Vec<PathBuf> = match &json["paths"] {
                Value::Array(a) => a.iter().filter_map(|x| x.as_str()).map(PathBuf::from).collect(),
                Value::String(s) => vec![PathBuf::from(s)],
                _ => vec![],
            };
            if paths.is_empty() {
                (change, None)
            } else {
                (change, Some(ClipRead::Files(paths)))
            }
        }
        Some("image") => {
            let png = json["png"]
                .as_str()
                .map(PathBuf::from)
                .unwrap_or_else(|| fallback_png.to_path_buf());
            if !png.is_file() {
                return (change, None);
            }
            let mime = json["mime"].as_str().unwrap_or("image/png").to_string();
            let width = json_u32(&json["width"]).unwrap_or(0);
            let height = json_u32(&json["height"]).unwrap_or(0);
            (change, Some(ClipRead::Image { png_path: png, mime, width, height }))
        }
        Some("text") => match json["text"].as_str() {
            Some(t) if !t.is_empty() => (change, Some(ClipRead::Text(t.to_string()))),
            _ => (change, None),
        },
        _ => (change, None),
    }
}

#[cfg(target_os = "macos")]
fn read_macos(last: i64) -> (i64, Option<ClipRead>) {

    if let Some(text) = read_text() {
        return (last, Some(ClipRead::Text(text)));
    }
    let out_png = temp_png_path();
    let output = exec::sync_cmd("osascript")
        .args(["-l", "JavaScript", "-e", MAC_READ_JS, "--"])
        .arg(last.to_string())
        .arg(&out_png)
        .stderr(Stdio::null())
        .output();
    let Ok(output) = output else {
        return (last, None);
    };
    if !output.status.success() {
        return (last, None);
    }
    parse_read(&output.stdout, &out_png, last)
}

#[cfg(target_os = "macos")]
const MAC_READ_JS: &str = r#"function run(a){ObjC.import("Foundation");ObjC.import("AppKit");var pb=$.NSPasteboard.generalPasteboard;var change=parseInt(pb.changeCount);var last=parseInt(a[0],10);if(change===last){return JSON.stringify({change:change,unchanged:true});}var outPng=a[1];var items=pb.pasteboardItems;var paths=[];if(!items.isNil()){for(var i=0;i<items.count;i++){var it=items.objectAtIndex(i);var s=it.stringForType("public.file-url");if(!s.isNil()){var u=$.NSURL.URLWithString(s);if(!u.isNil()&&u.isFileURL){paths.push(ObjC.unwrap(u.path));}}}}if(paths.length>0){return JSON.stringify({change:change,kind:"files",paths:paths});}var data=pb.dataForType("public.png");if(data.isNil()){data=pb.dataForType("public.tiff");}if(!data.isNil()){var rep=$.NSBitmapImageRep.imageRepWithData(data);if(!rep.isNil()){var png=rep.representationUsingTypeProperties(4,$());if(!png.isNil()){png.writeToFileAtomically($(outPng),true);return JSON.stringify({change:change,kind:"image",png:outPng,mime:"image/png",width:parseInt(rep.pixelsWide),height:parseInt(rep.pixelsHigh)});}}}var t=pb.stringForType("public.utf8-plain-text");if(!t.isNil()){return JSON.stringify({change:change,kind:"text",text:ObjC.unwrap(t)});}return JSON.stringify({change:change,kind:"empty"});}"#;

#[cfg(target_os = "macos")]
const MAC_WRITE_IMG_JS: &str = r#"function run(a){ObjC.import("Foundation");ObjC.import("AppKit");var d=$.NSData.dataWithContentsOfFile(a[0]);if(d.isNil()){return "err";}var pb=$.NSPasteboard.generalPasteboard;pb.clearContents;return pb.setDataForType(d,"public.png")?"ok":"err";}"#;

#[cfg(target_os = "macos")]
const MAC_WRITE_FILES_JS: &str = r#"function run(a){ObjC.import("Foundation");ObjC.import("AppKit");var pb=$.NSPasteboard.generalPasteboard;pb.clearContents;var urls=$.NSMutableArray.alloc.init;for(var i=0;i<a.length;i++){urls.addObject($.NSURL.fileURLWithPath(a[i]));}return pb.writeObjects(urls)?"ok":"err";}"#;

#[cfg(target_os = "macos")]
fn read_command() -> Option<Command> {
    Some(exec::sync_cmd("pbpaste"))
}

#[cfg(target_os = "macos")]
fn write_command() -> Option<Command> {
    Some(exec::sync_cmd("pbcopy"))
}

#[cfg(target_os = "windows")]
fn read_windows(last: i64) -> (i64, Option<ClipRead>) {
    let out_png = temp_png_path();
    let output = exec::sync_cmd("powershell")
        .args(["-Sta", "-NoProfile", "-Command", WIN_READ_PS])
        .env("RDT_LASTCHANGE", last.to_string())
        .env("RDT_OUTPNG", &out_png)
        .stderr(Stdio::null())
        .output();
    let Ok(output) = output else {
        return (last, None);
    };
    if !output.status.success() {
        return (last, None);
    }
    parse_read(&output.stdout, &out_png, last)
}

#[cfg(target_os = "windows")]
const WIN_READ_PS: &str = r#"Add-Type -Namespace Win32 -Name Clip -MemberDefinition '[DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();'
$seq = [long][Win32.Clip]::GetClipboardSequenceNumber()
$last = [long]$env:RDT_LASTCHANGE
if ($seq -eq $last) { '{"change":' + $seq + ',"unchanged":true}'; exit }
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$cb = [System.Windows.Forms.Clipboard]
if ($cb::ContainsFileDropList()) {
  $paths = @($cb::GetFileDropList())
  @{ change = $seq; kind = 'files'; paths = $paths } | ConvertTo-Json -Compress
} elseif ($cb::ContainsImage()) {
  $img = $cb::GetImage()
  $img.Save($env:RDT_OUTPNG, [System.Drawing.Imaging.ImageFormat]::Png)
  @{ change = $seq; kind = 'image'; png = $env:RDT_OUTPNG; mime = 'image/png'; width = $img.Width; height = $img.Height } | ConvertTo-Json -Compress
} elseif ($cb::ContainsText()) {
  @{ change = $seq; kind = 'text'; text = $cb::GetText() } | ConvertTo-Json -Compress
} else {
  @{ change = $seq; kind = 'empty' } | ConvertTo-Json -Compress
}"#;

#[cfg(target_os = "windows")]
const WIN_WRITE_IMG_PS: &str = r#"Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
try { $img = [System.Drawing.Image]::FromFile($env:RDT_CLIPPATH); [System.Windows.Forms.Clipboard]::SetImage($img); 'ok' } catch { 'err' }"#;

#[cfg(target_os = "windows")]
const WIN_WRITE_FILES_PS: &str = r#"Add-Type -AssemblyName System.Windows.Forms
try {
  $col = New-Object System.Collections.Specialized.StringCollection
  foreach ($p in ($env:RDT_CLIPPATHS -split "`n")) { if ($p) { [void]$col.Add($p) } }
  [System.Windows.Forms.Clipboard]::SetFileDropList($col); 'ok'
} catch { 'err' }"#;

#[cfg(target_os = "windows")]
fn read_command() -> Option<Command> {
    let mut c = exec::sync_cmd("powershell");
    c.args(["-NoProfile", "-Command", "Get-Clipboard -Raw"]);
    Some(c)
}

#[cfg(target_os = "windows")]
fn write_command() -> Option<Command> {
    let mut c = exec::sync_cmd("powershell");
    c.args(["-NoProfile", "-Command", "$input | Set-Clipboard"]);
    Some(c)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn read_command() -> Option<Command> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn write_command() -> Option<Command> {
    None
}

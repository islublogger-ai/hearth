//! Deliberately small tool surface. File operations use anchored directory fds,
//! openat/O_NOFOLLOW and immutable prepared arguments, never a shell.

use serde_json::{json, Map, Value};
use std::ffi::{CStr, CString, OsString};
use std::fs::{self, File, Metadata};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_WRITE_BYTES: usize = 64 * 1024;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 200;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct PreparedTool {
    pub name: String,
    pub args: Value,
    pub preview: String,
    pub needs_approval: bool,
    captured_name: String,
    captured_args: Value,
    captured_preview: String,
    captured_approval: bool,
    operation: Operation,
}

#[derive(Debug)]
enum Operation {
    Calculator(f64),
    List(Scope),
    Read {
        scope: Scope,
        leaf: CString,
        identity: Identity,
    },
    Write {
        scope: Scope,
        leaf: CString,
        before: Option<Snapshot>,
        content: Vec<u8>,
    },
}

#[derive(Debug)]
struct Scope {
    root_path: PathBuf,
    root: File,
    directory: File,
    components: Vec<OsString>,
}

#[derive(Debug, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    identity: Identity,
    bytes: Vec<u8>,
    mode: u32,
    modified: (i64, i64),
    changed: (i64, i64),
}

pub fn prepare(name: &str, args: &Value, workspace: &str) -> Result<PreparedTool, String> {
    let (args, preview, needs_approval, operation) = match name {
        "calculator" => {
            let fields = exact_fields(args, &["expression"])?;
            let expression = string_arg(fields, "expression")?;
            if expression.len() > 512 || expression.trim().is_empty() {
                return Err("Calculator expression must contain 1–512 bytes".into());
            }
            let value = meval::eval_str(expression)
                .map_err(|error| format!("Invalid calculator expression: {error}"))?;
            if !value.is_finite() {
                return Err("Calculator result must be a finite number".into());
            }
            (
                args.clone(),
                expression.to_owned(),
                false,
                Operation::Calculator(value),
            )
        }
        "list_dir" => {
            let fields = exact_fields(args, &["path"])?;
            let (root_path, parts, display) = resolve(workspace, string_arg(fields, "path")?)?;
            let scope = Scope::open(root_path, parts)?;
            (
                json!({"path":display}),
                format!("List {display}"),
                false,
                Operation::List(scope),
            )
        }
        "read_file" => {
            let fields = exact_fields(args, &["path"])?;
            let (root_path, mut parts, display) = resolve(workspace, string_arg(fields, "path")?)?;
            let leaf = leaf_name(&mut parts)?;
            let scope = Scope::open(root_path, parts)?;
            let before =
                snapshot(&scope.directory, &leaf, MAX_FILE_BYTES)?.ok_or("File does not exist")?;
            (
                json!({"path":display}),
                format!("Read {display}"),
                false,
                Operation::Read {
                    scope,
                    leaf,
                    identity: before.identity,
                },
            )
        }
        "write_file" => {
            let fields = exact_fields(args, &["path", "content"])?;
            let content = string_arg(fields, "content")?;
            if content.len() > MAX_WRITE_BYTES {
                return Err(format!("Writes are limited to {MAX_WRITE_BYTES} bytes"));
            }
            let (root_path, mut parts, display) = resolve(workspace, string_arg(fields, "path")?)?;
            let leaf = leaf_name(&mut parts)?;
            let scope = Scope::open(root_path, parts)?;
            // Both versions fit in the approval preview. Oversized existing files
            // are not replaced with an incomplete representation of old content.
            let before = snapshot(&scope.directory, &leaf, MAX_WRITE_BYTES)?;
            let preview = if let Some(before) = &before {
                let original = std::str::from_utf8(&before.bytes)
                    .map_err(|_| "Only UTF-8 text files can be overwritten")?;
                format!("Replace {display}\n\n--- Current complete contents\n{original}\n\n+++ Proposed complete contents\n{content}")
            } else {
                format!("Create {display}\n\n+++ Proposed complete contents\n{content}")
            };
            (
                json!({"path":display,"content":content}),
                preview,
                true,
                Operation::Write {
                    scope,
                    leaf,
                    before,
                    content: content.as_bytes().to_vec(),
                },
            )
        }
        _ => return Err(format!("Unknown or unavailable tool '{name}'")),
    };
    Ok(PreparedTool {
        name: name.to_owned(),
        captured_name: name.to_owned(),
        captured_args: args.clone(),
        captured_preview: preview.clone(),
        captured_approval: needs_approval,
        args,
        preview,
        needs_approval,
        operation,
    })
}

/// Caller must first obtain a run-bound approval for `needs_approval` operations.
/// Changing any displayed property invalidates this prepared operation.
pub fn execute(prepared: &PreparedTool) -> Result<String, String> {
    if prepared.name != prepared.captured_name
        || prepared.args != prepared.captured_args
        || prepared.preview != prepared.captured_preview
        || prepared.needs_approval != prepared.captured_approval
    {
        return Err(
            "Prepared operation changed; prepare it again and request a new approval".into(),
        );
    }
    match &prepared.operation {
        Operation::Calculator(value) => Ok(value.to_string()),
        Operation::List(scope) => {
            scope.verify()?;
            list_directory(&scope.directory)
        }
        Operation::Read {
            scope,
            leaf,
            identity,
        } => {
            scope.verify()?;
            let current = snapshot(&scope.directory, leaf, MAX_FILE_BYTES)?
                .ok_or("File was removed after preparation")?;
            if &current.identity != identity {
                return Err("File was replaced after preparation; request it again".into());
            }
            let content = String::from_utf8(current.bytes)
                .map_err(|_| "Only UTF-8 text files can be read")?;
            Ok(bound_output(content))
        }
        Operation::Write {
            scope,
            leaf,
            before,
            content,
        } => {
            scope.verify()?;
            let current = snapshot(&scope.directory, leaf, MAX_WRITE_BYTES)?;
            if &current != before {
                return Err("File changed since the approval preview; prepare a new write".into());
            }
            if before.as_ref().is_some_and(|old| old.bytes == *content) {
                return Ok("No changes; the file already contains the proposed text.".into());
            }
            let mut temp = TemporaryFile::create(&scope.directory)?;
            temp.file.write_all(content).map_err(io_error)?;
            let mode = before.as_ref().map_or(0o600, |old| old.mode & 0o777);
            if unsafe { libc::fchmod(temp.file.as_raw_fd(), mode as libc::mode_t) } != 0 {
                return Err(last_error("Could not set file permissions"));
            }
            temp.file.sync_all().map_err(io_error)?;
            // Revalidate after writing/syncing the temporary file. The current
            // snapshot contains full bytes and identity, not just a timestamp.
            scope.verify()?;
            if &snapshot(&scope.directory, leaf, MAX_WRITE_BYTES)? != before {
                return Err("File changed since the approval preview; prepare a new write".into());
            }
            scope.verify()?;
            let dir = scope.directory.as_raw_fd();
            let result = if before.is_none() {
                // Publishing a completed temporary inode with linkat is atomic
                // and never replaces an existing destination (including a
                // symlink). Drop subsequently removes the temporary name.
                unsafe { libc::linkat(dir, temp.name.as_ptr(), dir, leaf.as_ptr(), 0) }
            } else {
                // renameat does not follow a substituted target symlink. macOS
                // does not offer content compare-and-swap for existing files:
                // an unrelated writer can race this final compare/rename.
                unsafe { libc::renameat(dir, temp.name.as_ptr(), dir, leaf.as_ptr()) }
            };
            if result != 0 {
                return Err(last_error(
                    "Atomic save failed (the destination may have changed)",
                ));
            }
            if before.is_some() {
                temp.renamed = true;
            }
            // Some macOS filesystems do not support syncing directory handles.
            // The file itself was synchronized before publication.
            let _ = scope.directory.sync_all();
            Ok(format!(
                "Saved {} bytes to {}.",
                content.len(),
                prepared.args["path"].as_str().unwrap_or_default()
            ))
        }
    }
}

pub fn schema() -> Value {
    let mut alternatives = vec![json!({
        "type":"object", "properties":{"final":{"type":"string","minLength":1}},
        "required":["final"], "additionalProperties":false
    })];
    for (name, properties, required) in [
        (
            "calculator",
            json!({"expression":{"type":"string","maxLength":512}}),
            vec!["expression"],
        ),
        ("list_dir", json!({"path":{"type":"string"}}), vec!["path"]),
        ("read_file", json!({"path":{"type":"string"}}), vec!["path"]),
        (
            "write_file",
            json!({"path":{"type":"string"},"content":{"type":"string","maxLength":65536}}),
            vec!["path", "content"],
        ),
    ] {
        alternatives.push(json!({
            "type":"object", "properties":{
                "tool":{"type":"string","enum":[name]},
                "args":{"type":"object","properties":properties,"required":required,"additionalProperties":false}
            }, "required":["tool","args"], "additionalProperties":false
        }));
    }
    json!({"oneOf":alternatives})
}

fn exact_fields<'a>(args: &'a Value, allowed: &[&str]) -> Result<&'a Map<String, Value>, String> {
    let fields = args.as_object().ok_or("Tool arguments must be an object")?;
    if fields.len() != allowed.len() || allowed.iter().any(|key| !fields.contains_key(*key)) {
        return Err(format!(
            "Use exactly these arguments: {}",
            allowed.join(", ")
        ));
    }
    Ok(fields)
}

fn string_arg<'a>(fields: &'a Map<String, Value>, name: &str) -> Result<&'a str, String> {
    fields
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Argument '{name}' must be a string"))
}

fn resolve(workspace: &str, request: &str) -> Result<(PathBuf, Vec<OsString>, String), String> {
    if workspace.trim().is_empty() {
        return Err(
            "Choose an explicit workspace folder in Settings before using file tools".into(),
        );
    }
    let requested_root = Path::new(workspace);
    if !requested_root.is_absolute() {
        return Err("Workspace must be an existing absolute folder path".into());
    }
    if fs::symlink_metadata(requested_root)
        .map_err(io_error)?
        .file_type()
        .is_symlink()
    {
        return Err("A workspace cannot be a symlink; choose the actual folder".into());
    }
    let root = fs::canonicalize(requested_root).map_err(io_error)?;
    if request.len() > 4096 || request.contains('\0') {
        return Err("File path is invalid or too long".into());
    }
    let path = Path::new(request);
    let relative = if path.is_absolute() {
        path.strip_prefix(&root)
            .or_else(|_| path.strip_prefix(requested_root))
            .map_err(|_| "Path is outside the configured workspace")?
    } else {
        path
    };
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_os_string()),
            Component::CurDir => {}
            _ => {
                return Err("Parent traversal and paths outside the workspace are forbidden".into())
            }
        }
    }
    let display = parts
        .iter()
        .fold(root.clone(), |mut path, part| {
            path.push(part);
            path
        })
        .to_str()
        .ok_or("Tool paths must be valid UTF-8")?
        .to_owned();
    Ok((root, parts, display))
}

fn leaf_name(parts: &mut Vec<OsString>) -> Result<CString, String> {
    let leaf = parts
        .pop()
        .ok_or("A file path must name a file inside the workspace")?;
    CString::new(leaf.as_bytes()).map_err(|_| "Path contains a NUL byte".into())
}

impl Scope {
    fn open(root_path: PathBuf, components: Vec<OsString>) -> Result<Self, String> {
        let root = open_root(&root_path)?;
        let directory = walk(&root, &components)?;
        Ok(Self {
            root_path,
            root,
            directory,
            components,
        })
    }

    fn verify(&self) -> Result<(), String> {
        let named_root = open_root(&self.root_path)?;
        if identity(&named_root.metadata().map_err(io_error)?)
            != identity(&self.root.metadata().map_err(io_error)?)
        {
            return Err("Workspace was moved or replaced; prepare the operation again".into());
        }
        let named_directory = walk(&self.root, &self.components)?;
        if identity(&named_directory.metadata().map_err(io_error)?)
            != identity(&self.directory.metadata().map_err(io_error)?)
        {
            return Err(
                "Workspace directory was moved or replaced; prepare the operation again".into(),
            );
        }
        Ok(())
    }
}

fn open_root(path: &Path) -> Result<File, String> {
    let slash = CString::new("/").expect("literal");
    let fd = unsafe {
        libc::open(
            slash.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(last_error("Could not open filesystem root"));
    }
    let root = unsafe { File::from_raw_fd(fd) };
    let parts: Vec<_> = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(part) => Some(part.to_os_string()),
            _ => None,
        })
        .collect();
    walk(&root, &parts)
}

fn walk(root: &File, components: &[OsString]) -> Result<File, String> {
    let mut directory = root.try_clone().map_err(io_error)?;
    for part in components {
        let name = CString::new(part.as_bytes()).map_err(|_| "Path contains a NUL byte")?;
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(last_error(
                "Cannot enter folder (symlinks and missing folders are blocked)",
            ));
        }
        directory = unsafe { File::from_raw_fd(fd) };
    }
    Ok(directory)
}

fn identity(metadata: &Metadata) -> Identity {
    Identity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn snapshot(directory: &File, leaf: &CStr, limit: usize) -> Result<Option<Snapshot>, String> {
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            leaf.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOENT) {
            return Ok(None);
        }
        return Err(format!("Cannot open file (symlinks are blocked): {error}"));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() {
        return Err("Only regular text files are allowed".into());
    }
    if metadata.len() > limit as u64 {
        return Err(format!("File exceeds the {limit}-byte safety limit"));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > limit {
        return Err(format!("File exceeds the {limit}-byte safety limit"));
    }
    let after = file.metadata().map_err(io_error)?;
    if identity(&metadata) != identity(&after)
        || metadata.len() != after.len()
        || (metadata.mtime(), metadata.mtime_nsec()) != (after.mtime(), after.mtime_nsec())
        || (metadata.ctime(), metadata.ctime_nsec()) != (after.ctime(), after.ctime_nsec())
    {
        return Err("File changed while it was being read; prepare the operation again".into());
    }
    Ok(Some(Snapshot {
        identity: identity(&metadata),
        bytes,
        mode: metadata.mode(),
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
    }))
}

struct TemporaryFile<'a> {
    directory: &'a File,
    name: CString,
    file: File,
    renamed: bool,
}

impl<'a> TemporaryFile<'a> {
    fn create(directory: &'a File) -> Result<Self, String> {
        for _ in 0..64 {
            let number = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let name = CString::new(format!(".hearth-write-{}-{number}", std::process::id()))
                .expect("safe temp name");
            // C varargs promote macOS's u16 mode_t to an int.
            let fd = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600 as libc::c_int,
                )
            };
            if fd >= 0 {
                return Ok(Self {
                    directory,
                    name,
                    file: unsafe { File::from_raw_fd(fd) },
                    renamed: false,
                });
            }
            if std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                return Err(last_error("Could not create atomic-save temporary file"));
            }
        }
        Err("Could not reserve an atomic-save temporary file".into())
    }
}

impl Drop for TemporaryFile<'_> {
    fn drop(&mut self) {
        if !self.renamed {
            unsafe {
                libc::unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0);
            }
        }
    }
}

fn list_directory(directory: &File) -> Result<String, String> {
    let dot = CString::new(".").expect("literal");
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            dot.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(last_error("Could not open directory listing handle"));
    }
    let stream = unsafe { libc::fdopendir(fd) };
    if stream.is_null() {
        unsafe {
            libc::close(fd);
        }
        return Err(last_error("Could not list directory"));
    }
    struct DirectoryStream(*mut libc::DIR);
    impl Drop for DirectoryStream {
        fn drop(&mut self) {
            unsafe {
                libc::closedir(self.0);
            }
        }
    }
    let stream = DirectoryStream(stream);
    let mut entries = Vec::new();
    let mut truncated = false;
    loop {
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        if entries.len() == MAX_DIRECTORY_ENTRIES {
            truncated = true;
            break;
        }
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        let result = unsafe {
            libc::fstatat(
                directory.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        let kind = if result == 0 {
            let stat = unsafe { stat.assume_init() };
            match stat.st_mode & libc::S_IFMT {
                libc::S_IFDIR => "folder",
                libc::S_IFREG => "file",
                libc::S_IFLNK => "symlink (blocked)",
                _ => "special (blocked)",
            }
        } else {
            "unavailable"
        };
        entries.push(format!(
            "{kind}\t{}",
            serde_json::to_string(&name.to_string_lossy()).expect("string serializes")
        ));
    }
    entries.sort();
    let mut output = entries.join("\n");
    if truncated {
        output.push_str("\n[Listing limited to 200 entries]");
    }
    if output.is_empty() {
        output.push_str("[Empty folder]");
    }
    Ok(bound_output(output))
}

fn bound_output(mut text: String) -> String {
    if text.len() > MAX_OUTPUT_BYTES {
        const MARKER: &str = "\n[Output truncated at 64 KiB]";
        let mut boundary = MAX_OUTPUT_BYTES - MARKER.len();
        while !text.is_char_boundary(boundary) {
            boundary -= 1;
        }
        text.truncate(boundary);
        text.push_str(MARKER);
    }
    text
}

fn io_error(error: std::io::Error) -> String {
    error.to_string()
}
fn last_error(context: &str) -> String {
    format!("{context}: {}", std::io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    fn workspace(dir: &TempDir) -> String {
        fs::canonicalize(dir.path())
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn calculator_is_finite_and_arguments_are_exact() {
        let calc = prepare("calculator", &json!({"expression":"(2+3)*4"}), "").unwrap();
        assert_eq!(execute(&calc).unwrap(), "20");
        assert!(!calc.needs_approval);
        assert!(prepare("calculator", &json!({"expression":"1/0"}), "").is_err());
        assert!(prepare("calculator", &json!({"expression":"2+2","extra":true}), "").is_err());
        assert!(prepare("run_shell", &json!({"command":"pwd"}), "").is_err());
    }

    #[test]
    fn file_tools_require_explicit_scope_and_reject_traversal_and_symlinks() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let root = workspace(&dir);
        fs::write(outside.path().join("secret"), "private").unwrap();
        symlink(outside.path(), dir.path().join("escape")).unwrap();
        symlink(outside.path().join("secret"), dir.path().join("alias")).unwrap();
        assert!(prepare("read_file", &json!({"path":"alias"}), "").is_err());
        for path in [
            "../secret".to_owned(),
            outside.path().join("secret").to_str().unwrap().to_owned(),
            "escape/secret".into(),
            "alias".into(),
        ] {
            assert!(prepare("read_file", &json!({"path":path}), &root).is_err());
            assert!(prepare(
                "write_file",
                &json!({"path":path,"content":"unsafe"}),
                &root
            )
            .is_err());
        }
        assert!(prepare(
            "write_file",
            &json!({"path":"missing/new","content":"text"}),
            &root
        )
        .is_err());
        assert!(prepare("write_file", &json!({"path":".","content":"text"}), &root).is_err());
        assert_eq!(
            fs::read_to_string(outside.path().join("secret")).unwrap(),
            "private"
        );
    }

    #[test]
    fn atomic_create_overwrite_and_preview_are_bound() {
        let dir = TempDir::new().unwrap();
        let root = workspace(&dir);
        let create = prepare(
            "write_file",
            &json!({"path":"notes.md","content":"First 猫"}),
            &root,
        )
        .unwrap();
        assert!(create.needs_approval);
        assert!(create.preview.contains("First 猫"));
        execute(&create).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("notes.md")).unwrap(),
            "First 猫"
        );
        assert!(execute(&create).is_err());
        let overwrite = prepare(
            "write_file",
            &json!({"path":"notes.md","content":"Second"}),
            &root,
        )
        .unwrap();
        assert!(overwrite.preview.contains("First 猫"));
        assert!(overwrite.preview.contains("Second"));
        execute(&overwrite).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("notes.md")).unwrap(),
            "Second"
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        let read = prepare("read_file", &json!({"path":"notes.md"}), &root).unwrap();
        assert_eq!(execute(&read).unwrap(), "Second");
    }

    #[test]
    fn approval_cannot_change_args_preview_name_or_policy() {
        let dir = TempDir::new().unwrap();
        let root = workspace(&dir);
        for property in 0..4 {
            let mut write = prepare(
                "write_file",
                &json!({"path":"new","content":"approved"}),
                &root,
            )
            .unwrap();
            match property {
                0 => write.args["content"] = json!("different"),
                1 => write.preview = "different".into(),
                2 => write.name = "calculator".into(),
                _ => write.needs_approval = false,
            }
            assert!(execute(&write).is_err());
        }
        assert!(!dir.path().join("new").exists());
    }

    #[test]
    fn detects_edits_new_destinations_and_symlink_substitution_after_preview() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let root = workspace(&dir);
        fs::write(dir.path().join("existing"), "old").unwrap();
        let write = prepare(
            "write_file",
            &json!({"path":"existing","content":"proposed"}),
            &root,
        )
        .unwrap();
        fs::write(dir.path().join("existing"), "external edit").unwrap();
        assert!(execute(&write).is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("existing")).unwrap(),
            "external edit"
        );
        let create = prepare(
            "write_file",
            &json!({"path":"new","content":"proposed"}),
            &root,
        )
        .unwrap();
        fs::write(dir.path().join("new"), "external new file").unwrap();
        assert!(execute(&create).is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("new")).unwrap(),
            "external new file"
        );
        let read = prepare("read_file", &json!({"path":"existing"}), &root).unwrap();
        let write = prepare(
            "write_file",
            &json!({"path":"existing","content":"proposed"}),
            &root,
        )
        .unwrap();
        fs::write(outside.path().join("secret"), "private").unwrap();
        fs::remove_file(dir.path().join("existing")).unwrap();
        symlink(outside.path().join("secret"), dir.path().join("existing")).unwrap();
        assert!(execute(&read).is_err());
        assert!(execute(&write).is_err());
        assert_eq!(
            fs::read_to_string(outside.path().join("secret")).unwrap(),
            "private"
        );
    }

    #[test]
    fn rejects_relocated_or_symlink_replaced_parent_directory() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let root = workspace(&dir);
        fs::create_dir(dir.path().join("folder")).unwrap();
        let write = prepare(
            "write_file",
            &json!({"path":"folder/new","content":"approved"}),
            &root,
        )
        .unwrap();
        fs::rename(dir.path().join("folder"), outside.path().join("moved")).unwrap();
        symlink(outside.path().join("moved"), dir.path().join("folder")).unwrap();
        assert!(execute(&write).is_err());
        assert!(!outside.path().join("moved/new").exists());
    }

    #[test]
    fn oversized_content_and_special_files_are_blocked() {
        let dir = TempDir::new().unwrap();
        let root = workspace(&dir);
        assert!(prepare(
            "write_file",
            &json!({"path":"large","content":"x".repeat(MAX_WRITE_BYTES + 1)}),
            &root
        )
        .is_err());
        fs::write(dir.path().join("large"), vec![b'x'; MAX_FILE_BYTES + 1]).unwrap();
        assert!(prepare("read_file", &json!({"path":"large"}), &root).is_err());
        let fifo = CString::new(dir.path().join("pipe").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(prepare("read_file", &json!({"path":"pipe"}), &root).is_err());
        let listing = prepare("list_dir", &json!({"path":"."}), &root).unwrap();
        assert!(execute(&listing).unwrap().contains("special (blocked)"));
    }
}

use std::{path::PathBuf, sync::Arc};

use redis::{RedisResult, aio::MultiplexedConnection};
use serde::{Deserialize, Serialize};

use crate::config::IhaCdnConfig;

pub struct SharedState {
    pub config: Arc<IhaCdnConfig>,
    pub redis: Arc<redis::Client>,
}

impl SharedState {
    pub async fn make_connection(&self) -> RedisResult<MultiplexedConnection> {
        self.redis.get_multiplexed_async_connection().await
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum CDNData {
    Short {
        target: String,
    },
    File {
        is_admin: bool,
        path: PathBuf,
        mimetype: String,
        time_added: i64,
    },
    Code {
        is_admin: bool,
        path: PathBuf,
        mimetype: String,
        time_added: i64,
    },
}

pub const PREFIX: &str = "ihacdn";

pub const DELETED_ERROR: &'static str = r#"System.IO.FileNotFoundException: Could not find file '{{ FN }}' in server filesystem.
File name: '{{ FN }}'
   at System.IO.__Error.WinIOError(Int32 errorCode, String maybeFullPath)
   at System.IO.FileStream.Init(String path, FileMode mode, FileAccess access, Int32 rights, Boolean useRights, FileShare share, Int32 bufferSize, FileOptions options, SECURITY_ATTRIBUTES secAttrs, String msgPath, Boolean bFromProxy, Boolean useLongPath, Boolean checkHost)
   at System.IO.FileStream..ctor(String path, FileMode mode, FileAccess access, FileShare share, Int32 bufferSize, FileOptions options, String msgPath, Boolean bFromProxy, Boolean useLongPath, Boolean checkHost)
   at System.IO.StreamReader..ctor(String path, Encoding encoding, Boolean detectEncodingFromByteOrderMarks, Int32 bufferSize, Boolean checkHost)
   at System.IO.File.InternalReadAllText(String path, Encoding encoding, Boolean checkHost)
   at System.IO.File.ReadAllText(String path)
   at ConsoleApp.Program.Main(String[] args) in FileHandling.cs:line 182
"#;

pub const PAYLOAD_TOO_LARGE: &'static str = r"/usr/bin/../lib/gcc/x86_64/9.3-win32/../../../../usr/bin/as: ihaCDN/routes/FileHandler.o: too many sections (37616)
ihaCDN/request/upload/{{ FN }}: Assembler messages:
ihaCDN/request/upload/{{ FN }}: Fatal error: can't write ihaCDN/routes/FileHandler.o: File too big (Maximum allowed is {{ FS }})
";

pub const BLOCKED_EXTENSION: &'static str = r"[InvalidCastException: '{{ FILE_TYPE }}' is not allowed.]
ValidateExteension() in FileHandler.cs:65
ASP.UploadRoutes.Page_Load(Object sender, EventArgs e) in UploadRoutes.ascx:20
System.Web.Util.CalliHelper.EventArgFunctionCaller(IntPtr fp, Object o, Object t, EventArgs e) +15
System.Web.Util.CalliEventHandlerDelegateProxy.Callback(Object sender, EventArgs e) +36
System.Web.UI.Control.OnLoad(EventArgs e) +102
System.Web.UI.Control.LoadRecursive() +47
System.Web.UI.Control.LoadRecursive() +131
System.Web.UI.Control.LoadRecursive() +131
System.Web.UI.Page.ProcessRequestMain(Boolean includeStagesBeforeAsyncPoint, Boolean includeStagesAfterAsyncPoint) +1064
";

pub const MISSING_FIELD: &'static str = r#"Notice: Undefined index: file in /var/www/html/upload.php on line 17
Warning: file_get_contents(): "file" cannot be empty in /var/www/html/upload.php on line 18
"#;

pub const INVALID_URL_FORMAT: &'static str = r#"ValueError: Invalid URL format provided: '{{ URL }}'
  File "url_validator.py", line 42, in validate_url
    raise ValueError(f"Invalid URL format provided: '{url}'")
"#;

pub const REDIS_CONNECTION_ERROR: &'static str = r#"panic: Could not connect to Redis server. Connection failed.
goroutine 1 [running]:
main.connectToRedis(...)
    /opt/ihacdn/redis_client.go:34
main.initRedis()
    /opt/ihacdn/redis_client.go:21 +0x85
main.main()
    /opt/ihacdn/main.go:12 +0x39
exit status 2
"#;

pub const CREATE_FILE_ERROR: &'static str = r#"Errno::ENOENT: Failed to open and create file @ rb_sysopen - '{{ FN }}'
    from /usr/lib/ruby/3.0.0/open-uri.rb:37:in `read'
    from /usr/lib/ruby/3.0.0/open-uri.rb:37:in `open'
    from data_reader.rb:12:in `read_input'
"#;

pub const READ_FILE_ERROR: &'static str = r#"Errno::ENOENT: Failed to read file @ rb_sysopen - '{{ FN }}'
    from /usr/lib/ruby/3.0.0/open-uri.rb:37:in `read'
    from /usr/lib/ruby/3.0.0/open-uri.rb:37:in `open'
    from data_reader.rb:12:in `read_input'
"#;

pub const SAVE_FILE_ERROR: &'static str = r#"Exception in thread "main" java.io.IOException: Failed to save data to '{{ FN }}': {{ REASON }}
    at com.ihacdn.FileHandler.saveData(FileHandler.java:45)
    at com.ihacdn.Main.main(Main.java:12)
"#;

pub const REDIS_SAVE_ERROR: &'static str = r#"thread 'main' panicked at 'Failed to save data to Redis', src/redis_handler.rs:45:10
stack backtrace:
   0: ihacdn::redis_handler::save_data
             at src/redis_handler.rs:45
   1: ihacdn::main
             at src/main.rs:12
   2: std::rt::lang_start
             at /rustc/1.86.0/library/std/src/rt.rs:165
"#;

pub const REDIS_GET_ERROR: &'static str = r#"thread 'main' panicked at 'Failed to get data from Redis for `{{ FN }}`', src/redis_handler.rs:88:18
stack backtrace:
   0: ihacdn::redis_handler::get_data
             at src/redis_handler.rs:88
   1: ihacdn::main
             at src/main.rs:15
   2: std::rt::lang_start
             at /rustc/1.86.0/library/std/src/rt.rs:165
"#;

pub const CUSTOM_NAME_GENERATION_ERROR: &'static str = r#"Error: Failed to generate custom name: {{ REASON }}
    at generateCustomName (customNameGenerator.js:45:15)
    at processRequest (requestHandler.js:32:10)
    at async handleRequest (server.js:78:7)
"#;

const SUFFIXES: [&str; 11] = [
    "B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB", "RiB", "QiB",
];
const UNIT: f64 = 1024.0;

pub fn humanize_bytes(bytes: u64) -> String {
    let num_bytes = bytes as f64;

    if num_bytes < UNIT {
        format!("{} B", num_bytes as u16)
    } else {
        let mut result = String::new();
        let base = num_bytes.log2() as usize / 10;

        let curr_base = UNIT.powi(base as i32) as f64;

        let units = num_bytes / curr_base;
        let units = (units * 100.0).floor() / 100.0;
        let mut once = true;
        let extra = format!("{:.2}", units);
        let trimmed = extra
            .trim_end_matches(|_| {
                if once {
                    once = false;
                    true
                } else {
                    false
                }
            })
            .trim_end_matches("0")
            .trim_end_matches(".");

        result.push_str(trimmed);
        result.push_str(" ");
        result.push_str(SUFFIXES[base as usize]);
        result
    }
}

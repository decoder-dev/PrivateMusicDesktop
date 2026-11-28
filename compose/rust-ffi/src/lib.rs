use pm_engine::app::{App, AppOptions, RadioSource};
use pm_engine::audio::{output_devices, CpalSink, Sink};
use pm_engine::queue::Repeat;
use pm_engine::store::SecretStore;
#[cfg(target_os = "android")]
use pm_engine::store::MemoryStore;
#[cfg(not(target_os = "android"))]
use pm_engine::store::KeyringStore;
use pm_engine::vk::{Session, Track, VkClient};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct Engine {
    app: App,
    events: Arc<Mutex<VecDeque<String>>>,
    output_device: Arc<Mutex<Option<String>>>,
}

type Shared = Mutex<Engine>;

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum Command {
    PlayTracks { tracks: Vec<Track>, index: usize },
    PlayRadio { source: RadioSource, tracks: Vec<Track>, index: usize, next_offset: Option<u32> },
    Toggle,
    Next,
    Previous,
    Seek { seconds: f64 },
    SetVolume { volume: f32 },
    Jump { index: usize },
    PlayerState,
    SignIn { session: Session },
    SignOut,
    SessionState,
    Library { offset: u32, count: u32 },
    Search { query: String, offset: u32, count: u32 },
    CatalogMixes,
    StreamMix { mix_id: String, options: Option<String>, offset: u32 },
    Recommendations,
    NewReleases,
    Downloads,
    Download { track: Track },
    RemoveDownload { track_id: String },
    Lyrics { track: Track },
    AddToLibrary { track: Track },
    RemoveFromLibrary { track: Track },
    SetShuffle { on: bool },
    SetRepeat { repeat: Repeat },
    SetEqualizer { gains: Vec<f32> },
    OutputDevices,
    SetOutputDevice { name: Option<String> },
}

fn new_engine() -> Box<Shared> {
    new_engine_with_data_dir(None)
}

fn new_engine_with_data_dir(data_dir: Option<PathBuf>) -> Box<Shared> {
    let events = Arc::new(Mutex::new(VecDeque::new()));
    let event_sink = events.clone();
    let output_device = Arc::new(Mutex::new(None::<String>));
    let device_for_sink = output_device.clone();
    #[cfg(not(target_os = "android"))]
    let store: Box<dyn SecretStore> = Box::new(KeyringStore::new("PrivateMusicDesktop"));
    #[cfg(target_os = "android")]
    let store: Box<dyn SecretStore> = Box::<MemoryStore>::default();
    let data_dir = data_dir
        .or_else(|| std::env::var_os("PM_DATA_DIR").map(PathBuf::from))
        .or_else(|| std::env::var_os("APPDATA").map(|path| PathBuf::from(path).join("PrivateMusicDesktop")))
        .or_else(|| std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".local/share/private-music")))
        .unwrap_or_else(|| std::env::temp_dir().join("private-music"));
    let app = App::new(
        VkClient::new().expect("built-in VK endpoint"),
        move || {
            let name = device_for_sink.lock().unwrap_or_else(|poison| poison.into_inner()).clone();
            CpalSink::open(name.as_deref()).map(|sink| Box::new(sink) as Box<dyn Sink>)
        },
        store,
        move |event| {
            if let Ok(raw) = serde_json::to_string(&event) {
                event_sink.lock().unwrap_or_else(|poison| poison.into_inner()).push_back(raw);
            }
        },
        AppOptions {
            offline_dir: Some(data_dir.join("downloads")),
            history_dir: Some(data_dir.join("history")),
            watch_network: false,
            ..AppOptions::default()
        },
    );
    Box::new(Mutex::new(Engine { app, events, output_device }))
}

fn command(engine: &mut Engine, raw: &str) -> String {
    let result = (|| -> Result<Value, String> {
        let command: Command = serde_json::from_str(raw).map_err(|error| error.to_string())?;
        let app = &engine.app;
        let controller = app.controller();
        match command {
            Command::PlayTracks { tracks, index } => controller.play_tracks(tracks, index),
            Command::PlayRadio { source, tracks, index, next_offset } => app.play_radio_from(source, tracks, index, next_offset).map_err(|error| error.to_string())?,
            Command::Toggle => controller.toggle(),
            Command::Next => controller.next(),
            Command::Previous => controller.previous(),
            Command::Seek { seconds } => controller.seek(seconds),
            Command::SetVolume { volume } => controller.set_volume(volume),
            Command::Jump { index } => controller.jump(index),
            Command::PlayerState => return serde_json::to_value(controller.state()).map_err(|error| error.to_string()),
            Command::SignIn { session } => app.sign_in(session),
            Command::SignOut => app.sign_out(),
            Command::SessionState => return serde_json::to_value(app.session_state()).map_err(|error| error.to_string()),
            Command::Library { offset, count } => return serde_json::to_value(app.library(offset, count).map_err(|error| error.to_string())?).map_err(|error| error.to_string()),
            Command::Search { query, offset, count } => return serde_json::to_value(app.search(&query, offset, count).map_err(|error| error.to_string())?).map_err(|error| error.to_string()),
            Command::CatalogMixes => return serde_json::to_value(app.catalog_mixes().map_err(|error| error.to_string())?).map_err(|error| error.to_string()),
            Command::StreamMix { mix_id, options, offset } => return serde_json::to_value(app.stream_mix_at(&mix_id, options.as_deref(), offset).map_err(|error| error.to_string())?).map_err(|error| error.to_string()),
            Command::Recommendations => return serde_json::to_value(app.recommendations().map_err(|error| error.to_string())?).map_err(|error| error.to_string()),
            Command::NewReleases => return serde_json::to_value(app.new_releases().map_err(|error| error.to_string())?).map_err(|error| error.to_string()),
            Command::Downloads => return serde_json::to_value(app.downloads()).map_err(|error| error.to_string()),
            Command::Download { track } => return serde_json::to_value(app.download(&track).map_err(|error| error.to_string())?).map_err(|error| error.to_string()),
            Command::RemoveDownload { track_id } => return serde_json::to_value(app.remove_download(&track_id).map_err(|error| error.to_string())?).map_err(|error| error.to_string()),
            Command::Lyrics { track } => return serde_json::to_value(app.lyrics(&track).map_err(|error| error.to_string())?).map_err(|error| error.to_string()),
            Command::AddToLibrary { track } => return serde_json::to_value(app.add_to_library(&track).map_err(|error| error.to_string())?).map_err(|error| error.to_string()),
            Command::RemoveFromLibrary { track } => app.remove_from_library(&track).map_err(|error| error.to_string())?,
            Command::SetShuffle { on } => controller.set_shuffle(on),
            Command::SetRepeat { repeat } => controller.set_repeat(repeat),
            Command::SetEqualizer { gains } => {
                let gains: [f32; 10] = gains.try_into().map_err(|_| "equalizer needs exactly 10 gains".to_string())?;
                controller.set_equalizer(Some(gains));
            }
            Command::OutputDevices => return serde_json::to_value(output_devices()).map_err(|error| error.to_string()),
            Command::SetOutputDevice { name } => {
                *engine.output_device.lock().unwrap_or_else(|poison| poison.into_inner()) = name;
                controller.reopen_output();
            }
        }
        Ok(Value::Null)
    })();
    match result {
        Ok(value) => value.to_string(),
        Err(error) => json!({ "error": error }).to_string(),
    }
}

fn into_c(text: String) -> *mut c_char {
    CString::new(text.replace('\0', " ")).expect("NUL was removed").into_raw()
}

unsafe fn engine_ref<'a>(pointer: *mut c_void) -> Option<&'a Shared> {
    pointer.cast::<Shared>().as_ref()
}

#[no_mangle]
pub extern "C" fn pm_engine_new() -> *mut c_void {
    Box::into_raw(new_engine()).cast()
}

#[no_mangle]
pub unsafe extern "C" fn pm_engine_free(pointer: *mut c_void) {
    if !pointer.is_null() {
        drop(Box::from_raw(pointer.cast::<Shared>()));
    }
}

#[no_mangle]
pub unsafe extern "C" fn pm_engine_command(pointer: *mut c_void, raw: *const c_char) -> *mut c_char {
    let Some(engine) = engine_ref(pointer) else { return into_c(json!({"error":"engine is closed"}).to_string()) };
    if raw.is_null() { return into_c(json!({"error":"missing command"}).to_string()) }
    let raw = CStr::from_ptr(raw).to_string_lossy();
    let result = command(&mut engine.lock().unwrap_or_else(|poison| poison.into_inner()), &raw);
    into_c(result)
}

#[no_mangle]
pub unsafe extern "C" fn pm_engine_poll_event(pointer: *mut c_void) -> *mut c_char {
    let Some(engine) = engine_ref(pointer) else { return std::ptr::null_mut() };
    let events = {
        let engine = engine.lock().unwrap_or_else(|poison| poison.into_inner());
        engine.events.clone()
    };
    let next = events.lock().unwrap_or_else(|poison| poison.into_inner()).pop_front();
    next.map_or(std::ptr::null_mut(), into_c)
}

#[no_mangle]
pub unsafe extern "C" fn pm_string_free(pointer: *mut c_char) {
    if !pointer.is_null() {
        drop(CString::from_raw(pointer));
    }
}

#[cfg(target_os = "android")]
mod android {
    use super::{command, new_engine_with_data_dir, Shared};
    use jni::objects::{GlobalRef, JClass, JObject, JString};
    use jni::sys::{jlong, jstring};
    use jni::JNIEnv;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    // CPAL uses this application context to query the Android audio service.
    // Keep its JNI global reference alive for the whole process.
    static APPLICATION_CONTEXT: OnceLock<GlobalRef> = OnceLock::new();

    fn pointer(engine: jlong) -> *mut super::c_void {
        engine as usize as *mut super::c_void
    }

    #[no_mangle]
    pub extern "system" fn Java_com_dec_privatemusic_player_RustPlayerApiJni_pm_1engine_1new(
        mut env: JNIEnv,
        _class: JClass,
        context: JObject,
        data_dir: JString,
    ) -> jlong {
        APPLICATION_CONTEXT.get_or_init(|| {
            let vm = env.get_java_vm().expect("Android Java VM");
            let context = env.new_global_ref(context).expect("Android application context");
            unsafe {
                ndk_context::initialize_android_context(vm.get_java_vm_pointer().cast(), context.as_obj().as_raw().cast());
            }
            context
        });
        let path: String = env.get_string(&data_dir).expect("UTF-8 data directory").into();
        Box::into_raw(new_engine_with_data_dir(Some(PathBuf::from(path)))) as usize as jlong
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_dec_privatemusic_player_RustPlayerApiJni_pm_1engine_1free(
        _env: JNIEnv,
        _class: JClass,
        engine: jlong,
    ) {
        if engine != 0 {
            drop(Box::from_raw(pointer(engine).cast::<Mutex<super::Engine>>()));
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_dec_privatemusic_player_RustPlayerApiJni_pm_1engine_1command(
        mut env: JNIEnv,
        _class: JClass,
        engine: jlong,
        raw: JString,
    ) -> jstring {
        let raw: String = env.get_string(&raw).expect("UTF-8 command").into();
        let engine = &*pointer(engine).cast::<Shared>();
        let result = command(&mut engine.lock().unwrap_or_else(|poison| poison.into_inner()), &raw);
        env.new_string(result).expect("UTF-8 response").into_raw()
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_dec_privatemusic_player_RustPlayerApiJni_pm_1engine_1poll_1event(
        env: JNIEnv,
        _class: JClass,
        engine: jlong,
    ) -> jstring {
        let engine = &*pointer(engine).cast::<Shared>();
        let events = {
            let engine = engine.lock().unwrap_or_else(|poison| poison.into_inner());
            engine.events.clone()
        };
        let next = events.lock().unwrap_or_else(|poison| poison.into_inner()).pop_front();
        match next {
            Some(raw) => env.new_string(raw).expect("UTF-8 event").into_raw(),
            None => std::ptr::null_mut(),
        }
    }
}

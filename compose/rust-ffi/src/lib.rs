use pm_engine::app::{App, AppOptions};
use pm_engine::audio::{CpalSink, Sink};
use pm_engine::store::MemoryStore;
use pm_engine::vk::{Track, VkClient};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::sync::{Arc, Mutex};

struct Engine {
    app: App,
    events: Arc<Mutex<VecDeque<String>>>,
}

type Shared = Mutex<Engine>;

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum Command {
    PlayTracks { tracks: Vec<Track>, index: usize },
    Toggle,
    Next,
    Previous,
    Seek { seconds: f64 },
    SetVolume { volume: f32 },
    Jump { index: usize },
    PlayerState,
}

fn new_engine() -> Box<Shared> {
    let events = Arc::new(Mutex::new(VecDeque::new()));
    let event_sink = events.clone();
    let app = App::new(
        VkClient::new().expect("built-in VK endpoint"),
        || CpalSink::open(None).map(|sink| Box::new(sink) as Box<dyn Sink>),
        Box::<MemoryStore>::default(),
        move |event| {
            if let Ok(raw) = serde_json::to_string(&event) {
                event_sink.lock().unwrap_or_else(|poison| poison.into_inner()).push_back(raw);
            }
        },
        AppOptions { watch_network: false, ..AppOptions::default() },
    );
    Box::new(Mutex::new(Engine { app, events }))
}

fn command(engine: &mut Engine, raw: &str) -> String {
    let result = (|| -> Result<Value, String> {
        let command: Command = serde_json::from_str(raw).map_err(|error| error.to_string())?;
        let controller = engine.app.controller();
        match command {
            Command::PlayTracks { tracks, index } => controller.play_tracks(tracks, index),
            Command::Toggle => controller.toggle(),
            Command::Next => controller.next(),
            Command::Previous => controller.previous(),
            Command::Seek { seconds } => controller.seek(seconds),
            Command::SetVolume { volume } => controller.set_volume(volume),
            Command::Jump { index } => controller.jump(index),
            Command::PlayerState => return serde_json::to_value(controller.state()).map_err(|error| error.to_string()),
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
    use super::{command, new_engine, Shared};
    use jni::objects::{JClass, JString};
    use jni::sys::{jlong, jstring};
    use jni::JNIEnv;
    use std::sync::Mutex;

    fn pointer(engine: jlong) -> *mut super::c_void {
        engine as usize as *mut super::c_void
    }

    #[no_mangle]
    pub extern "system" fn Java_com_dec_privatemusic_player_RustPlayerApiJni_pm_1engine_1new(
        _env: JNIEnv,
        _class: JClass,
    ) -> jlong {
        Box::into_raw(new_engine()) as usize as jlong
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
        mut env: JNIEnv,
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

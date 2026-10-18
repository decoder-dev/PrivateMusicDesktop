//! Жизнь в сети, которая меняется под действием приложения (VPN включается или выключается, Wi-Fi
//! подключается к кабелю, ноутбук просыпается в другом месте).
//!
//! VK связывает токены, поток URLs и файлы cookie с сетью, в которой находился клиент, а пуловое
//! соединение поддержания активности, которое было открыто до изменения, впоследствии становится
//! неработающим сокетом. С этой задачей справляются три небольших кусочка:
//!
//! * [`Pool`]: общий клиент `reqwest`, который перестраивается (отбрасывается каждое соединение в пуле)
//!   при следующем использовании после [`drop_connections`];
//! * [`Tracker`] + [`NetworkMonitor`]: обратите внимание, что маршрут компьютера к Интернету изменился.
//!   Пакет не отправляется: при подключении к сокету UDP OS только запрашивается, какой локальный адрес
//!   он будет использовать, поэтому проверка занимает несколько микросекунд, и поток монитора засыпает
//!   навсегда, когда ничего не воспроизводится и окно не сфокусировано;
//! * [`backoff`]: ограниченные экспоненциальные задержки с джиттером.

use std::net::{IpAddr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use serde::Serialize;

use crate::{Error, Result};

// ------------------------------------------------------------ соединения

/// Натолкнулся на [`drop_connections`]; каждая [`Pool`] и каждая загрузка потока сравнивают его со
/// значением, на котором он был создан.
static EPOCH: AtomicU64 = AtomicU64::new(0);

/// Текущее поколение соединения.
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Acquire)
}

/// Заставляет каждого общего клиента HTTP начинать заново: следующий запрос открывает новые соединения
/// (новый поиск DNS, новый сеанс TLS) вместо повторного использования сокета, принадлежащего старой
/// сети. Дешево: ничего не восстанавливается, пока не будет использовано.
pub fn drop_connections() {
    EPOCH.fetch_add(1, Ordering::AcqRel);
}

type Build = dyn Fn() -> Result<Client> + Send + Sync;

/// Клиент `reqwest`, который можно выбросить и пересобрать, которым пользуются клоны.
pub struct Pool {
    build: Box<Build>,
    current: Mutex<(u64, Client)>,
}

impl Pool {
    pub fn new(build: impl Fn() -> Result<Client> + Send + Sync + 'static) -> Result<Self> {
        let client = build()?;
        Ok(Self { build: Box::new(build), current: Mutex::new((epoch(), client)) })
    }

    /// Клиент, который нужно использовать сейчас; его клон — дешевая ручка на одном пуле.
    pub fn get(&self) -> Client {
        let mut slot = self.current.lock().unwrap_or_else(|p| p.into_inner());
        let now = epoch();
        if slot.0 != now {
            match (self.build)() {
                Ok(fresh) => {
                    let old = std::mem::replace(&mut slot.1, fresh);
                    slot.0 = now;
                    // Удаление блокирующего клиента присоединяется к его потоку времени выполнения:
                    // держите его подальше от потока вызывающего (возможно, асинхронного).
                    let _ = std::thread::Builder::new().name("pm-net-drop".into()).spawn(move || drop(old));
                }
                // Оставьте старый клиент; попробуйте еще раз при следующем запросе.
                Err(error) => log::warn!("could not rebuild the HTTP client: {}", error.kind()),
            }
        }
        slot.1.clone()
    }
}

/// `Pool::new` для обычного случая: общий пул за `Arc`.
pub fn shared_pool(build: impl Fn() -> Result<Client> + Send + Sync + 'static) -> Result<Arc<Pool>> {
    Pool::new(build).map(Arc::new)
}

/// Верно для ошибок с надписью "the connection or the route is bad", а не "the server answered no".
pub fn is_connectivity(error: &Error) -> bool {
    matches!(error, Error::Timeout | Error::Offline | Error::Transport(_))
}

/// Ошибка I/O при чтении тела ответа, в тексте которого нет ничего из запроса (со знаком URL).
pub fn read_error(error: &std::io::Error) -> Error {
    if error.kind() == std::io::ErrorKind::TimedOut {
        Error::Timeout
    } else {
        Error::Transport("connection lost while reading".into())
    }
}

/// Число секунд ожидания перед повторной попыткой `attempt` (от 0): `base · 2^attempt` ограничено
/// значением `cap`, с разбросом на ±`spread` (доля) с `unit` в диапазоне 0…1 и никогда не ниже 0.
pub fn backoff(attempt: u32, base: f64, cap: f64, spread: f64, unit: f64) -> f64 {
    let unit = if unit.is_finite() { unit.clamp(0.0, 1.0) } else { 0.5 };
    let raw = (base * 2f64.powi(attempt.min(16) as i32)).min(cap);
    (raw * (1.0 - spread + 2.0 * spread * unit)).clamp(0.0, cap)
}

/// Случайное значение 0…1 для джиттера.
pub fn jitter_unit() -> f64 {
    use rand::Rng;
    rand::thread_rng().gen::<f64>()
}

// ------------------------------------------------------------------ изменить

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NetState {
    /// Соединение есть и стабильное (также: оно только что вернулось).
    Ok,
    /// Маршрут к Интернету изменился; объединенные соединения были удалены.
    Changed,
    /// Маршрута нет вообще.
    Unreachable,
}

/// Какие локальные адреса OS будет использовать для выхода в Интернет. Два зонда для IPv4 (общий адрес
/// и VK, поскольку разделенный туннель VPNs маршрутизирует диапазоны VK отдельно) и один для IPv6, где
/// учитывается только /64: конфиденциальные адреса меняются внутри него без каких-либо изменений в
/// сети.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fingerprint {
    pub v4: Option<IpAddr>,
    pub v4_vk: Option<IpAddr>,
    pub v6: Option<IpAddr>,
}

impl Fingerprint {
    pub fn reachable(&self) -> bool {
        self.v4.is_some() || self.v4_vk.is_some() || self.v6.is_some()
    }
}

/// Откуда берутся отпечатки пальцев; тесты вводят свои собственные.
pub trait AddressSource: Send + Sync {
    fn read(&self) -> Fingerprint;
}

/// Операционная система через UDP `connect` (которая ничего не отправляет).
pub struct SystemAddresses;

impl SystemAddresses {
    fn local_for(bind: &str, target: &str) -> Option<IpAddr> {
        let socket = UdpSocket::bind(bind).ok()?;
        socket.connect(target.parse::<SocketAddr>().ok()?).ok()?;
        let ip = socket.local_addr().ok()?.ip();
        (!ip.is_unspecified()).then_some(ip)
    }
}

impl AddressSource for SystemAddresses {
    fn read(&self) -> Fingerprint {
        let v6 = Self::local_for("[::]:0", "[2606:4700:4700::1111]:443").map(|ip| match ip {
            IpAddr::V6(v6) => {
                let mut octets = v6.octets();
                octets[8..].fill(0);
                IpAddr::V6(Ipv6Addr::from(octets))
            }
            other => other,
        });
        Fingerprint {
            v4: Self::local_for("0.0.0.0:0", "1.1.1.1:443"),
            // Внутри собственного диапазона VK; используется только для того, чтобы спросить, какой
            // маршрут подойдет.
            v4_vk: Self::local_for("0.0.0.0:0", "87.240.137.164:443"),
            v6,
        }
    }
}

/// Конечная машина за монитором: дайте ей отпечатки пальцев, она скажет, что сказать окну. Об изменении
/// объявляется сразу (`Changed`) и оно называется `Ok` только тогда, когда при следующем просмотре оно
/// обнаруживается без изменений, поэтому VPN, который изменяется в течение нескольких секунд во время
/// подключения, не вызывает повторную попытку для каждого изменения.
#[derive(Debug, Default)]
pub struct Tracker {
    last: Option<Fingerprint>,
    awaiting_stable: bool,
}

impl Tracker {
    pub fn observe(&mut self, now: Fingerprint) -> Vec<NetState> {
        let Some(previous) = self.last.replace(now) else {
            // Первый взгляд: базовый уровень, а не изменение.
            return if now.reachable() { Vec::new() } else { vec![NetState::Unreachable] };
        };
        if previous == now {
            return if std::mem::take(&mut self.awaiting_stable) && now.reachable() { vec![NetState::Ok] } else { Vec::new() };
        }
        if now.reachable() {
            self.awaiting_stable = true;
            vec![NetState::Changed]
        } else {
            self.awaiting_stable = false;
            vec![NetState::Unreachable]
        }
    }

    /// Было объявлено об изменении, и его подтверждение должно быть скоро.
    pub fn awaiting_stable(&self) -> bool {
        self.awaiting_stable
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MonitorConfig {
    /// Между взглядами, пока что-то воспроизводится или окно сфокусировано.
    pub poll: Duration,
    /// Между изменением и взглядом, который его подтверждает.
    pub confirm: Duration,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self { poll: Duration::from_secs(8), confirm: Duration::from_millis(2500) }
    }
}

#[derive(Default)]
struct Flags {
    playing: bool,
    focused: bool,
    force: bool,
    stop: bool,
}

struct Shared {
    flags: Mutex<Flags>,
    wake: Condvar,
}

/// Просматривает маршрут каждые несколько секунд, но только пока это важно.
pub struct NetworkMonitor {
    handle: MonitorHandle,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// Чем звонящие управляют монитором; дешево клонировать.
#[derive(Clone)]
pub struct MonitorHandle {
    shared: Arc<Shared>,
}

impl std::ops::Deref for NetworkMonitor {
    type Target = MonitorHandle;
    fn deref(&self) -> &MonitorHandle {
        &self.handle
    }
}

impl NetworkMonitor {
    pub fn spawn(source: Arc<dyn AddressSource>, config: MonitorConfig, on_state: impl Fn(NetState) + Send + 'static) -> Self {
        let shared = Arc::new(Shared { flags: Mutex::new(Flags::default()), wake: Condvar::new() });
        let inner = shared.clone();
        let thread = std::thread::Builder::new()
            .name("pm-net".into())
            .spawn(move || run(&inner, source.as_ref(), config, &on_state))
            .ok();
        Self { handle: MonitorHandle { shared }, thread: Mutex::new(thread) }
    }

    pub fn handle(&self) -> MonitorHandle {
        self.handle.clone()
    }
}

impl MonitorHandle {
    fn update(&self, f: impl FnOnce(&mut Flags)) {
        f(&mut self.shared.flags.lock().unwrap_or_else(|p| p.into_inner()));
        self.shared.wake.notify_all();
    }

    /// Звук воспроизводится (или загружается): продолжайте смотреть.
    pub fn set_playing(&self, playing: bool) {
        let mut flags = self.shared.flags.lock().unwrap_or_else(|p| p.into_inner());
        if flags.playing != playing {
            flags.playing = playing;
            self.shared.wake.notify_all();
        }
    }

    /// Окно привлекает внимание пользователя: продолжайте смотреть.
    pub fn set_focused(&self, focused: bool) {
        let mut flags = self.shared.flags.lock().unwrap_or_else(|p| p.into_inner());
        if flags.focused != focused {
            flags.focused = focused;
            self.shared.wake.notify_all();
        }
    }

    /// Посмотрите прямо сейчас, даже если ничего не воспроизводится (пользователь нажал «Повторить»).
    pub fn check_now(&self) {
        self.update(|f| f.force = true);
    }
}

impl Drop for NetworkMonitor {
    fn drop(&mut self) {
        self.handle.update(|f| f.stop = true);
        if let Some(thread) = self.thread.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = thread.join();
        }
    }
}

fn run(shared: &Shared, source: &dyn AddressSource, config: MonitorConfig, on_state: &dyn Fn(NetState)) {
    let mut tracker = Tracker::default();
    let mut next_look = Instant::now();
    let mut was_active = false;
    let mut flags = shared.flags.lock().unwrap_or_else(|p| p.into_inner());
    loop {
        if flags.stop {
            return;
        }
        let active = flags.playing || flags.focused;
        if !active && !flags.force {
            // Не за чем следить: спите, пока не скажут иначе, никаких пробуждений.
            was_active = false;
            flags = shared.wake.wait(flags).unwrap_or_else(|p| p.into_inner());
            continue;
        }
        if active && !was_active {
            // Сеть могла измениться, пока никто не смотрел.
            was_active = true;
            flags.force = true;
        }
        if !flags.force {
            let wait = next_look.saturating_duration_since(Instant::now());
            if !wait.is_zero() {
                flags = shared.wake.wait_timeout(flags, wait).unwrap_or_else(|p| p.into_inner()).0;
                continue;
            }
        }
        flags.force = false;
        drop(flags);
        for state in tracker.observe(source.read()) {
            on_state(state);
        }
        next_look = Instant::now() + if tracker.awaiting_stable() { config.confirm } else { config.poll };
        flags = shared.flags.lock().unwrap_or_else(|p| p.into_inner());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn ip(text: &str) -> Option<IpAddr> {
        Some(text.parse().unwrap())
    }

    fn print(v4: &str) -> Fingerprint {
        Fingerprint { v4: ip(v4), v4_vk: ip(v4), v6: None }
    }

    const NONE: Fingerprint = Fingerprint { v4: None, v4_vk: None, v6: None };

    #[test]
    fn the_first_look_is_a_baseline_not_a_change() {
        let mut tracker = Tracker::default();
        assert!(tracker.observe(print("192.168.1.5")).is_empty());
        assert!(tracker.observe(print("192.168.1.5")).is_empty());
        assert!(Tracker::default().observe(NONE) == vec![NetState::Unreachable]);
    }

    #[test]
    fn a_new_address_is_a_change_and_is_confirmed_once_stable() {
        let mut tracker = Tracker::default();
        tracker.observe(print("192.168.1.5"));
        assert_eq!(tracker.observe(print("10.8.0.2")), vec![NetState::Changed]);
        assert!(tracker.awaiting_stable());
        assert_eq!(tracker.observe(print("10.8.0.2")), vec![NetState::Ok]);
        assert!(!tracker.awaiting_stable());
        assert!(tracker.observe(print("10.8.0.2")).is_empty(), "Ok is said once");
    }

    #[test]
    fn a_flapping_vpn_is_not_ok_until_it_holds_still() {
        let mut tracker = Tracker::default();
        tracker.observe(print("192.168.1.5"));
        assert_eq!(tracker.observe(print("10.8.0.2")), vec![NetState::Changed]);
        assert_eq!(tracker.observe(print("192.168.1.5")), vec![NetState::Changed]);
        assert_eq!(tracker.observe(print("10.8.0.2")), vec![NetState::Changed]);
        assert_eq!(tracker.observe(print("10.8.0.2")), vec![NetState::Ok]);
    }

    #[test]
    fn losing_and_regaining_the_route() {
        let mut tracker = Tracker::default();
        tracker.observe(print("192.168.1.5"));
        assert_eq!(tracker.observe(NONE), vec![NetState::Unreachable]);
        assert!(tracker.observe(NONE).is_empty(), "no repeat while it stays down");
        assert_eq!(tracker.observe(print("192.168.1.5")), vec![NetState::Changed], "the old sockets are dead even for the same address");
        assert_eq!(tracker.observe(print("192.168.1.5")), vec![NetState::Ok]);
    }

    #[test]
    fn a_route_only_for_vk_changing_counts() {
        let mut tracker = Tracker::default();
        let a = Fingerprint { v4: ip("192.168.1.5"), v4_vk: ip("192.168.1.5"), v6: None };
        let b = Fingerprint { v4: ip("192.168.1.5"), v4_vk: ip("10.8.0.2"), v6: None };
        tracker.observe(a);
        assert_eq!(tracker.observe(b), vec![NetState::Changed]);
    }

    #[test]
    fn backoff_grows_is_capped_and_jittered_within_bounds() {
        assert_eq!(backoff(0, 1.0, 20.0, 0.0, 0.5), 1.0);
        assert_eq!(backoff(1, 1.0, 20.0, 0.0, 0.5), 2.0);
        assert_eq!(backoff(10, 1.0, 20.0, 0.0, 0.5), 20.0);
        let low = backoff(2, 1.0, 20.0, 0.15, 0.0);
        let high = backoff(2, 1.0, 20.0, 0.15, 1.0);
        assert!((3.4..3.41).contains(&low) && (4.59..4.61).contains(&high), "{low} {high}");
        assert!(backoff(40, 1.0, 20.0, 0.15, 1.0) <= 20.0);
        assert!(backoff(0, 1.0, 20.0, 0.15, f64::NAN).is_finite());
    }

    struct Script {
        prints: Mutex<Vec<Fingerprint>>,
        reads: AtomicUsize,
    }

    impl AddressSource for Script {
        fn read(&self) -> Fingerprint {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let mut prints = self.prints.lock().unwrap();
            if prints.len() > 1 { prints.remove(0) } else { prints[0] }
        }
    }

    fn wait_for(what: &str, f: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !f() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_monitor_reports_changes_from_an_injected_source_and_sleeps_when_idle() {
        let source = Arc::new(Script {
            prints: Mutex::new(vec![print("192.168.1.5"), print("10.8.0.2")]),
            reads: AtomicUsize::new(0),
        });
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let config = MonitorConfig { poll: Duration::from_millis(20), confirm: Duration::from_millis(10) };
        let monitor = NetworkMonitor::spawn(source.clone(), config, move |state| log.lock().unwrap().push(state));

        // Ни играя, ни сосредоточенно: ни единого взгляда.
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(source.reads.load(Ordering::SeqCst), 0);

        monitor.set_playing(true);
        wait_for("the change and its confirmation", || seen.lock().unwrap().len() >= 2);
        assert_eq!(*seen.lock().unwrap(), vec![NetState::Changed, NetState::Ok]);

        // Опять бездействие: взгляд прекращается.
        monitor.set_playing(false);
        std::thread::sleep(Duration::from_millis(60));
        let reads = source.reads.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(source.reads.load(Ordering::SeqCst), reads);

        // Явная проверка работает даже в режиме ожидания.
        monitor.check_now();
        wait_for("a forced look", || source.reads.load(Ordering::SeqCst) > reads);
    }

    #[test]
    fn the_pool_rebuilds_after_connections_are_dropped() {
        let built = Arc::new(AtomicUsize::new(0));
        let counter = built.clone();
        let pool = Pool::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Client::builder().build().map_err(Error::from)
        })
        .unwrap();
        pool.get();
        pool.get();
        assert_eq!(built.load(Ordering::SeqCst), 1);
        drop_connections();
        pool.get();
        pool.get();
        assert_eq!(built.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn the_system_source_gives_a_fingerprint_without_sending_anything() {
        // Работает с сетью или без нее; он просто не должен паниковать или зависать.
        let print = SystemAddresses.read();
        let _ = print.reachable();
    }
}

//! Политики автономной библиотеки, определяющие, какой сохраненный аудиофайл следует удалить. В
//! магазинах хранятся файлы и манифест; этот модуль отвечает только на "which of these records should
//! go first", поэтому правило имеет один проверенный дом.

use core::slice;

/// Один сохраненный трек, как его видит политика выселения.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct pm_eviction_candidate {
    /// Секунды с исходной даты, которую приложение использует для `lastPlayedAt`.
    pub last_played_at: f64,
    pub byte_count: i64,
    /// Автоматически кэшируемая запись, которая не является воспроизводимой в данный момент дорожкой.
    /// Сохраненные вручную треки и защищенные треки никогда не подлежат удалению.
    pub evictable: bool,
}

/// Выбирает выселяемых кандидатов, которые, по крайней мере, недавно играли первыми, до тех пор, пока
/// не будет выпущен хотя бы `required_bytes`. Связи сохраняют порядок ввода. Записывает выбранные
/// индексы кандидатов в `out` в порядке вытеснения (до `out_capacity`) и возвращает количество
/// выбранных. Ничего не выбирается, если `required_bytes` не ​​положителен; если выбраны все
/// вытесняемые кандидаты, а цель все еще не достигнута, это случай "limit reached" вызывающей стороны.
pub fn select_evictions(candidates: &[pm_eviction_candidate], required_bytes: i64) -> Vec<usize> {
    if required_bytes <= 0 {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..candidates.len()).filter(|&i| candidates[i].evictable).collect();
    // `total_cmp` помещает NaN последним, поэтому поврежденная временная метка удаляется последней.
    order.sort_by(|&a, &b| candidates[a].last_played_at.total_cmp(&candidates[b].last_played_at));
    let mut released: i64 = 0;
    let mut chosen = Vec::new();
    for index in order {
        chosen.push(index);
        released = released.saturating_add(candidates[index].byte_count);
        if released >= required_bytes {
            break;
        }
    }
    chosen
}

/// C оболочка ABI; см. [`select_evictions`].
#[no_mangle]
pub unsafe extern "C" fn pm_offline_select_evictions(
    candidates: *const pm_eviction_candidate,
    count: i32,
    required_bytes: i64,
    out_indices: *mut i32,
    out_capacity: i32,
) -> i32 {
    if candidates.is_null() || count <= 0 || out_indices.is_null() || out_capacity <= 0 {
        return 0;
    }
    let chosen = select_evictions(slice::from_raw_parts(candidates, count as usize), required_bytes);
    let out = slice::from_raw_parts_mut(out_indices, out_capacity as usize);
    let n = chosen.len().min(out.len());
    for (slot, index) in out.iter_mut().zip(&chosen) {
        *slot = *index as i32;
    }
    n as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(last: f64, bytes: i64, evictable: bool) -> pm_eviction_candidate {
        pm_eviction_candidate { last_played_at: last, byte_count: bytes, evictable }
    }

    #[test]
    fn evicts_oldest_automatic_entries_until_enough_is_released() {
        let items = [c(30.0, 10, true), c(10.0, 10, true), c(20.0, 10, true), c(5.0, 10, false)];
        assert_eq!(select_evictions(&items, 15), vec![1, 2]);
        assert_eq!(select_evictions(&items, 10), vec![1]);
        assert_eq!(select_evictions(&items, 31), vec![1, 2, 0]);
    }

    #[test]
    fn never_touches_manual_or_protected_tracks_and_may_fall_short() {
        let items = [c(1.0, 100, false), c(2.0, 100, false)];
        assert!(select_evictions(&items, 50).is_empty());
        let mixed = [c(1.0, 5, true), c(0.0, 500, false)];
        assert_eq!(select_evictions(&mixed, 50), vec![0]);
    }

    #[test]
    fn nothing_to_do_without_a_positive_target() {
        let items = [c(1.0, 10, true)];
        assert!(select_evictions(&items, 0).is_empty());
        assert!(select_evictions(&items, -5).is_empty());
        assert!(select_evictions(&[], 5).is_empty());
    }

    #[test]
    fn ties_keep_input_order_and_corrupt_timestamps_go_last() {
        let items = [c(5.0, 1, true), c(5.0, 1, true), c(f64::NAN, 1, true), c(1.0, 1, true)];
        assert_eq!(select_evictions(&items, 4), vec![3, 0, 1, 2]);
    }

    #[test]
    fn zero_sized_records_do_not_stop_the_scan_early_and_sums_do_not_overflow() {
        let items = [c(1.0, 0, true), c(2.0, i64::MAX, true), c(3.0, i64::MAX, true)];
        assert_eq!(select_evictions(&items, 10), vec![0, 1]);
        assert_eq!(select_evictions(&items, i64::MAX), vec![0, 1]);
    }

    #[test]
    fn c_abi_truncates_to_the_output_capacity() {
        let items = [c(1.0, 1, true), c(2.0, 1, true), c(3.0, 1, true)];
        let mut out = [-1i32; 2];
        let n = unsafe { pm_offline_select_evictions(items.as_ptr(), 3, 3, out.as_mut_ptr(), 2) };
        assert_eq!((n, out), (2, [0, 1]));
    }
}

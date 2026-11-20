//! The System page's words and decisions, without a window.

use hardware_core::battery::{BatteryFacts, BatteryStatus};
use hardware_core::charge::{ChargeChoice, charge_limit_view};
use hardware_core::power::{Caution, Degradation, Hold};
use hardware_core::sensors::{SensorKind, SensorLabel, SensorRow, SensorSummary};
use hardware_core::write::WriteOutcome;

use super::copy::copy;
use super::decide::*;
use crate::attention::{AttentionAction, Snapshot};
use crate::i18n::Locale;
use crate::pages::states::PageState;

const BOTH: [Locale; 2] = [Locale::EnUs, Locale::ZhTw];

#[test]
fn the_spec_lines_are_word_for_word() {
    let en = copy(Locale::EnUs);
    let zh = copy(Locale::ZhTw);
    assert_eq!(
        en.empty,
        "This PC has no hardware settings Better OS can change."
    );
    assert_eq!(zh.empty, "這台電腦沒有 Better OS 能調整的硬體設定。");
    assert_eq!(zh.follow_header, "依充電狀態自動切換電源模式");
    assert_eq!(en.follow_header, "Switch power mode with the charger");
    assert_eq!(zh.charge_80, "充到 80%（建議長時間接電使用）");
    assert_eq!(
        en.fan_after_restart,
        "It always goes back to Automatic when the PC restarts."
    );
    assert_eq!(zh.about, "裝置規格");
    // Neutral about restarts until the owner has seen one (ticket 86).
    assert!(!en.charge_at_startup.contains("100%"));
    assert!(!zh.charge_at_startup.contains("100%"));
}

#[test]
fn power_mode_cautions_read_in_both_languages() {
    let held = Caution::Held(Hold {
        application: "GNOME Boxes".into(),
        profile: "performance".into(),
    });
    assert_eq!(
        caution_line(&held, copy(Locale::EnUs)),
        "GNOME Boxes is asking for Best performance."
    );
    assert_eq!(
        caution_line(&held, copy(Locale::ZhTw)),
        "GNOME Boxes 正在要求「最佳效能」模式。"
    );
    assert_eq!(
        caution_line(
            &Caution::Degraded(Degradation::LapDetected),
            copy(Locale::ZhTw)
        ),
        "電腦放在腿上時，效能模式會降低以免過熱。"
    );
    assert_eq!(
        caution_line(
            &Caution::Degraded(Degradation::Other("x".into())),
            copy(Locale::EnUs)
        ),
        "Performance is reduced for now (x)."
    );
}

#[test]
fn a_limit_set_elsewhere_names_its_value() {
    let view = charge_limit_view(75);
    let label = charge_label(view.choices[view.selected], copy(Locale::EnUs));
    assert_eq!(label, "Charge to 75% (set elsewhere)");
    assert_eq!(
        charge_label(ChargeChoice::Limit(60), copy(Locale::ZhTw)),
        "充到 60%（電池壽命最長）"
    );
}

#[test]
fn the_battery_reads_its_charge_status_and_rows() {
    let battery = BatteryFacts {
        capacity: Some(87),
        status: BatteryStatus::Charging,
        full: Some(4_106_000.0),
        full_design: Some(4_680_000.0),
        cycles: Some(0),
        model: Some("A32-K55".into()),
    };
    assert_eq!(battery_description(&battery, Locale::ZhTw), "87% · 充電中");
    assert_eq!(
        battery_description(&battery, Locale::EnUs),
        "87% · Charging"
    );
    assert_eq!(
        battery_rows(&battery, copy(Locale::EnUs)),
        vec![
            "Battery health: holds 88% of its original capacity".to_string(),
            "Model: A32-K55".to_string()
        ]
    );
    let unread = BatteryFacts {
        capacity: None,
        status: BatteryStatus::Unknown,
        ..battery
    };
    assert_eq!(battery_description(&unread, Locale::EnUs), "Unknown");
}

#[test]
fn keyboard_levels_read_off_and_level_n() {
    let c = copy(Locale::ZhTw);
    assert_eq!(keyboard_value(0, c), "關閉");
    assert_eq!(keyboard_value(2, c), "第 2 段");
    let levels = hardware_core::display::KeyboardLevels::from_gnome(0, 4).unwrap();
    assert_eq!(
        keyboard_description(&levels, copy(Locale::EnUs)),
        "3 levels; 0 is off."
    );
}

#[test]
fn sensor_words_and_the_summary() {
    let c = copy(Locale::EnUs);
    let temp = SensorRow {
        label: SensorLabel::Processor,
        kind: SensorKind::Temperature,
        value: Some(52.4),
    };
    let fan = SensorRow {
        label: SensorLabel::CpuFan,
        kind: SensorKind::Fan,
        value: Some(4_800.0),
    };
    assert_eq!(sensor_value(&temp, c), "52 °C");
    assert_eq!(sensor_value(&fan, c), "4,800 RPM");
    assert_eq!(
        sensor_value(
            &SensorRow {
                value: None,
                ..temp.clone()
            },
            c
        ),
        "Can't be read right now"
    );
    let both = SensorSummary {
        hottest: Some((SensorLabel::Processor, 52.4)),
        fan_rpm: Some(4_800.0),
    };
    assert_eq!(
        sensors_description(&both, c),
        "Processor 52 °C · Fan 4,800 RPM"
    );
    assert_eq!(
        sensors_description(&both, copy(Locale::ZhTw)),
        "處理器 52 °C · 風扇 4,800 RPM"
    );
    let fan_only = SensorSummary {
        hottest: None,
        fan_rpm: Some(950.0),
    };
    assert_eq!(
        sensors_description(&fan_only, copy(Locale::ZhTw)),
        "風扇 950 RPM"
    );
    let temp_only = SensorSummary {
        hottest: Some((SensorLabel::Raw("spd5118".into()), 40.0)),
        fan_rpm: None,
    };
    assert_eq!(sensors_description(&temp_only, c), "spd5118 40 °C");
    assert_eq!(
        sensor_label(&SensorLabel::WiFi, copy(Locale::ZhTw)),
        "無線網路"
    );
}

#[test]
fn each_write_outcome_draws_its_state() {
    let en = Locale::EnUs;
    assert_eq!(outcome_state(&WriteOutcome::Done, en), None);
    assert_eq!(
        outcome_state(&WriteOutcome::Cancelled, en),
        Some(PageState::NoPermission)
    );
    assert_eq!(
        outcome_state(&WriteOutcome::NotAllowed, en),
        Some(PageState::NotAllowed)
    );
    assert_eq!(
        outcome_state(&WriteOutcome::ReadBack("100".into()), en),
        Some(PageState::ReadBackDifferent {
            value: "100".into()
        })
    );
    assert_eq!(
        outcome_state(
            &WriteOutcome::Failed {
                details: "org.freedesktop.DBus.Error.Failed: daemon.error.write_failed:EROFS"
                    .into()
            },
            en
        ),
        Some(PageState::WriteFailed {
            reason: "The hardware didn't accept the new value.".into()
        })
    );
    assert_eq!(
        outcome_state(
            &WriteOutcome::Failed {
                details: "org.freedesktop.DBus.Error.ServiceUnknown: no".into()
            },
            Locale::ZhTw
        ),
        Some(PageState::WriteFailed {
            reason: "負責變更這項設定的 Better OS 服務沒有回應。".into()
        })
    );
    assert_eq!(
        outcome_state(
            &WriteOutcome::Failed {
                details: "odd".into()
            },
            en
        ),
        Some(PageState::WriteFailed {
            reason: String::new()
        })
    );
    // The cancelled prompt's words, from the shared states (spec section 2).
    assert_eq!(
        crate::pages::states::describe(Locale::EnUs, &PageState::NoPermission).text,
        "Nothing changed: authentication wasn't completed."
    );
}

#[test]
fn the_desktop_line_is_gnome_with_its_version_and_the_session() {
    assert_eq!(
        desktop_line(Some("zorin:GNOME"), Some("46"), Some("wayland")).as_deref(),
        Some("GNOME 46 · Wayland")
    );
    assert_eq!(
        desktop_line(Some("ubuntu:GNOME"), None, Some("x11")).as_deref(),
        Some("GNOME · X11")
    );
    assert_eq!(
        desktop_line(Some("KDE"), Some("46"), None).as_deref(),
        Some("KDE")
    );
    assert_eq!(desktop_line(None, None, Some("tty")), None);
}

#[test]
fn about_rows_say_what_could_not_be_read() {
    let rows = about_rows(&AboutFacts::default(), Locale::EnUs);
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|(_, value)| value == "Couldn't be read"));
    let rows = about_rows(
        &AboutFacts {
            product: Some("ASUS Vivobook S 14 M5406WA".into()),
            ..Default::default()
        },
        Locale::ZhTw,
    );
    assert_eq!(
        rows[0],
        ("裝置名稱", "ASUS Vivobook S 14 M5406WA".to_string())
    );
}

#[test]
fn home_raises_a_worn_battery_and_nothing_else() {
    for locale in BOTH {
        let mut snapshot = Snapshot::empty(locale);
        assert!((super::PAGE.attention)(&snapshot).is_empty());
        snapshot.battery_health = Some(80);
        assert!((super::PAGE.attention)(&snapshot).is_empty());
        snapshot.battery_health = Some(72);
        let items = (super::PAGE.attention)(&snapshot);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].action, AttentionAction::OpenSystemBattery);
        assert!(items[0].description.contains("72"));
    }
    let mut snapshot = Snapshot::empty(Locale::ZhTw);
    snapshot.battery_health = Some(72);
    let items = (super::PAGE.attention)(&snapshot);
    assert_eq!(items[0].header, "電池老化");
    assert_eq!(items[0].description, "目前最多只能充到出廠容量的 72%。");
}

#[test]
fn every_card_is_findable_in_both_languages() {
    for locale in BOTH {
        let entries = (super::PAGE.search_entries)(locale);
        // Ten hardware cards (the keyboard colour is ticket 136's) and the
        // Drives section (ticket 127).
        assert_eq!(entries.len(), 11);
        assert!(entries.iter().any(|e| e.card == super::drives::CARD));
        for entry in &entries {
            assert_eq!(entry.route, crate::navigation::Route::System);
        }
    }
    let en = (super::PAGE.search_entries)(Locale::EnUs);
    assert!(
        crate::search::find(&en, "charge limit")
            .iter()
            .any(|e| e.card == super::card::CHARGE_LIMIT)
    );
    let zh = (super::PAGE.search_entries)(Locale::ZhTw);
    assert!(
        crate::search::find(&zh, "風扇")
            .iter()
            .any(|e| e.card == super::card::FAN_MODE)
    );
    assert!(
        crate::search::find(&en, "keyboard colour")
            .iter()
            .any(|e| e.card == super::card::LIGHTING)
    );
    assert!(
        crate::search::find(&zh, "鍵盤顏色")
            .iter()
            .any(|e| e.card == super::card::LIGHTING)
    );
}

#[test]
fn sentences_are_joined_the_way_each_language_writes_them() {
    assert_eq!(sentences(Locale::EnUs, &["One.", "Two.", ""]), "One. Two.");
    assert_eq!(sentences(Locale::ZhTw, &["一。", "二。"]), "一。二。");
}

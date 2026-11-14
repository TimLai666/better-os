//! The live pages: Overview, Processes, Details, Performance, Startup apps,
//! Services, and Settings.
//!
//! The pages read models and draw them. What a number means, what a row
//! says, and which words describe the machine are decided in `charts`,
//! `tables`, `startup`, and `i18n`, none of which knows about GPUI.

use better_ui::fluent::chart::{
    ChartHue, ChartSeries, CompositionBar, LineChart, nice_maximum, peak,
};
use better_ui::fluent::info_bar::{InfoBar, Severity};
use better_ui::fluent::list::{CellTone, Column, DetailsTable, SortState, TableCell};
use better_ui::fluent::selection::{ComboBox, ComboItem};
use better_ui::fluent::settings_card::{SettingsCard, SettingsExpander, SettingsRow, card_fill};
use better_ui::fluent::tabs::{SelectorBar, SelectorItem};
use better_ui::fluent::theme::Appearance;
use better_ui::fluent::toggle::ToggleSwitch;
use better_ui::fluent::{
    BODY, BODY_STRONG, CAPTION, Interaction, Palette, SUBTITLE, label, palette, radius, spacing,
    text, with_alpha,
};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{Icon, IconName};
use monitor_core::CollectorHealth;
use monitor_views::overview::ResourceVerdict;
use monitor_views::{ProcessColumn, SortDirection};

use crate::app::{MenuTarget, MonitorApp, Page, Resource};
use crate::charts::{AdapterKind, CAPACITY, SeriesKey};
use crate::i18n::{self, Copy, copy};
use crate::layout::card_columns;
use crate::shell::{card, grid, note_card, section_title, stat_or_missing};
use crate::tables::{
    AppColumn, AppLine, AppSelection, app_cells, app_lines, app_row, byte_axis, bytes,
    detail_columns, detail_width, numeric, process_cell, rate, share,
};

/// The tint a status word is drawn in, always beside the word.
fn verdict_color(verdict: &ResourceVerdict, p: &Palette) -> Rgba {
    match verdict {
        ResourceVerdict::Saturated { .. } | ResourceVerdict::CollectorFailed { .. } => p.critical,
        ResourceVerdict::UnderPressure { .. } => p.caution,
        _ => p.text_secondary,
    }
}

fn hertz(value: f64) -> String {
    format!("{:.2} GHz", value / 1e9)
}

impl MonitorApp {
    pub(crate) fn render_page(
        &mut self,
        available: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.page {
            Page::Overview => self.overview_page(available, cx),
            Page::Processes => self.processes_page(cx),
            Page::Details => self.details_page(cx),
            Page::Performance => self.performance_page(available, cx),
            Page::Startup => self.startup_page(cx),
            Page::Services => self.services_page(cx),
            Page::History => self.history_page(available, cx),
            Page::Events => self.events_page(cx),
            Page::SystemInfo => self.system_info_page(cx),
            Page::Settings => self.settings_page(window, cx),
        }
    }

    // Overview.

    fn overview_page(&mut self, available: f32, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let top = self.apps.top_by_cpu(5);
        let busiest = top
            .first()
            .filter(|row| row.cpu_utilization.total >= 0.05)
            .map(|row| row.group.display_name.clone());
        let (tone, headline, detail) = i18n::health_sentence(
            &self.overview,
            &self.readings,
            busiest.as_deref(),
            self.locale,
        );
        let tone_color = match tone {
            i18n::Tone::Good => p.success,
            i18n::Tone::Busy => p.accent,
            i18n::Tone::Strained => p.caution,
            i18n::Tone::Unknown => p.text_secondary,
        };
        let sentence = card(p).child(
            div()
                .flex()
                .items_start()
                .gap(spacing::MD)
                .child(
                    div()
                        .mt(px(6.0))
                        .size(px(12.0))
                        .flex_none()
                        .rounded_full()
                        .bg(tone_color),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .gap(px(4.0))
                        .child(label(headline, SUBTITLE, p.text_primary))
                        .children(detail.map(|detail| label(detail, BODY, p.text_secondary))),
                ),
        );

        let mut cards = vec![
            self.cpu_card(cx),
            self.memory_card(cx),
            self.disk_card(cx),
            self.network_card(cx),
        ];
        // A machine without a graphics card or a battery has no card for it,
        // as the Performance page and the popup have no tile for it.
        let shown = crate::charts::overview_hardware(&self.readings);
        if shown.gpu {
            cards.push(self.gpu_card(cx));
        }
        if shown.battery {
            cards.push(self.battery_card(cx));
        }

        let processors = self.processors();
        let rows: Vec<Vec<TableCell>> = top
            .iter()
            .map(|row| {
                vec![
                    TableCell::new(row.group.display_name.clone()),
                    crate::tables::cpu_aggregate(&row.cpu_utilization, c, processors),
                    crate::tables::aggregate(&row.memory_resident, c, bytes),
                ]
            })
            .collect();
        let using_most = card(p)
            .child(label(c.using_the_most, BODY_STRONG, p.text_primary))
            .child(
                DetailsTable::new(
                    "top-apps",
                    vec![
                        Column::new(c.column_name).sortable(false),
                        Column::new(c.column_cpu)
                            .width(px(96.0))
                            .align_end()
                            .sortable(false),
                        Column::new(c.column_memory)
                            .width(px(120.0))
                            .align_end()
                            .sortable(false),
                    ],
                    rows,
                )
                .presorted()
                .empty(c.waiting_for_first_reading),
            );

        div()
            .flex()
            .flex_col()
            .gap(spacing::LG)
            .child(sentence)
            .child(grid(card_columns(available), cards))
            .child(using_most)
            .into_any_element()
    }

    /// A card on the Overview: its name and status, the figure, a sparkline,
    /// and one line of detail. The whole card opens the resource's page.
    #[allow(clippy::too_many_arguments)]
    fn resource_card(
        &self,
        id: &'static str,
        title: &'static str,
        icon: IconName,
        status: Option<(String, Rgba)>,
        figure: Option<String>,
        chart: Option<(Vec<ChartSeries>, f64)>,
        detail: String,
        resource: Resource,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let rest = card_fill(true, Interaction::Rest, p);
        let hover = card_fill(true, Interaction::PointerOver, p);
        let pressed = card_fill(true, Interaction::Pressed, p);
        let open = Self::act(cx, move |this, _, cx| {
            this.show_resource(resource.clone(), cx)
        });
        let chart = match chart {
            Some((series, maximum)) => LineChart::sparkline(id, series, CAPACITY, maximum)
                .height(px(48.0))
                .into_any_element(),
            None => div()
                .h(px(48.0))
                .flex()
                .items_center()
                .child(label(c.no_chart, CAPTION, p.text_secondary))
                .into_any_element(),
        };
        div()
            .id(id)
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .p(spacing::LG)
            .min_w_0()
            .h_full()
            .rounded(radius::SM)
            .border_1()
            .border_color(p.card_stroke)
            .bg(rest)
            .hover(move |s| s.bg(hover))
            .active(move |s| s.bg(pressed))
            .cursor_pointer()
            .on_click(move |_, window, cx| open(window, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::SM)
                    .min_w_0()
                    .child(Icon::new(icon).size(px(16.0)).text_color(p.text_secondary))
                    .child(label(title, BODY_STRONG, p.text_primary).flex_1().min_w_0())
                    .children(status.map(|(word, color)| {
                        text(div(), CAPTION)
                            .flex_none()
                            .text_color(color)
                            .child(word)
                    })),
            )
            .child(match figure {
                Some(figure) => label(figure, SUBTITLE, p.text_primary),
                None => label(c.not_available, SUBTITLE, p.text_secondary),
            })
            .child(chart)
            .child(label(detail, CAPTION, p.text_secondary).truncate())
            .into_any_element()
    }

    fn cpu_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let cpu = &self.readings.cpu;
        let verdict = &self.overview.cpu.verdict;
        let detail = match (cpu.speed_hz, cpu.logical) {
            (Some(speed), Some(logical)) => format!(
                "{} · {}",
                hertz(speed),
                i18n::logical_processors(logical, self.locale)
            ),
            (Some(speed), None) => hertz(speed),
            (None, Some(logical)) => i18n::logical_processors(logical, self.locale),
            (None, None) => c.not_available.to_string(),
        };
        self.resource_card(
            "card-cpu",
            c.cpu,
            IconName::Cpu,
            Some((
                i18n::verdict_word(verdict, self.locale).to_string(),
                verdict_color(verdict, p),
            )),
            cpu.utilization.map(share),
            self.readings.has_cpu.then(|| {
                (
                    vec![ChartSeries::new(
                        self.live.values(&SeriesKey::Cpu),
                        ChartHue::Blue,
                    )],
                    100.0,
                )
            }),
            detail,
            Resource::Cpu,
            cx,
        )
    }

    fn memory_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let memory = &self.readings.memory;
        let verdict = &self.overview.memory.resource.verdict;
        let detail = match (memory.in_use(), memory.total) {
            (Some(used), Some(total)) => {
                i18n::of_total(&bytes(used as f64), &bytes(total as f64), self.locale)
            }
            _ => c.not_available.to_string(),
        };
        self.resource_card(
            "card-memory",
            c.memory,
            IconName::MemoryStick,
            Some((
                i18n::verdict_word(verdict, self.locale).to_string(),
                verdict_color(verdict, p),
            )),
            memory.in_use_fraction().map(share),
            self.readings.has_memory.then(|| {
                (
                    vec![ChartSeries::new(
                        self.live.values(&SeriesKey::Memory),
                        ChartHue::Purple,
                    )],
                    memory.total.unwrap_or(1) as f64,
                )
            }),
            detail,
            Resource::Memory,
            cx,
        )
    }

    fn disk_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let verdict = &self.overview.io.verdict;
        let (read, write) = self.readings.disk_total();
        let detail = i18n::read_write(read.map(rate), write.map(rate), self.locale);
        let first = self.readings.disks.first().map(|disk| disk.name.clone());
        let chart = first.clone().map(|name| {
            (
                vec![ChartSeries::new(
                    self.live.values(&SeriesKey::DiskActive(name)),
                    ChartHue::Green,
                )],
                100.0,
            )
        });
        self.resource_card(
            "card-disk",
            c.disk,
            IconName::HardDrive,
            Some((
                i18n::verdict_word(verdict, self.locale).to_string(),
                verdict_color(verdict, p),
            )),
            self.readings
                .busiest_disk_active()
                .map(|active| i18n::active_share(&share(active), self.locale)),
            chart,
            detail,
            first.map(Resource::Disk).unwrap_or(Resource::Cpu),
            cx,
        )
    }

    fn network_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let (receive, send) = self.readings.network_total();
        let adapter = self
            .readings
            .adapters
            .iter()
            .find(|adapter| adapter.connected == Some(true))
            .or(self.readings.adapters.first());
        let chart = adapter.map(|adapter| {
            let series = vec![
                ChartSeries::new(
                    self.live.values(&SeriesKey::Receive(adapter.name.clone())),
                    ChartHue::Orange,
                ),
                ChartSeries::new(
                    self.live.values(&SeriesKey::Send(adapter.name.clone())),
                    ChartHue::Orange,
                ),
            ];
            let top = byte_axis(peak(&series).unwrap_or(0.0), 1024.0);
            (series, top)
        });
        let figure = match (receive, send) {
            (Some(r), Some(s)) => Some(rate(r + s)),
            (one, other) => one.or(other).map(rate),
        };
        let detail = i18n::receive_send(receive.map(rate), send.map(rate), self.locale);
        let status = adapter.map(|adapter| {
            let p = palette(cx);
            (
                i18n::adapter_status(adapter.connected, self.locale).to_string(),
                p.text_secondary,
            )
        });
        self.resource_card(
            "card-network",
            c.network,
            IconName::Network,
            status,
            figure,
            chart,
            detail,
            adapter
                .map(|adapter| Resource::Adapter(adapter.name.clone()))
                .unwrap_or(Resource::Cpu),
            cx,
        )
    }

    fn gpu_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let Some(gpu) = self.readings.primary_gpu().cloned() else {
            // No card read: the inventory may still name one.
            let detail = i18n::graphics_cards(self.inventory.as_ref(), self.locale)
                .first()
                .map(|(card, _)| card.clone())
                .unwrap_or_else(|| c.gpu_not_found.to_string());
            return self.resource_card(
                "card-gpu",
                c.gpu,
                IconName::Frame,
                None,
                None,
                None,
                detail,
                Resource::Cpu,
                cx,
            );
        };
        let mut parts = Vec::new();
        if let (Some(used), Some(total)) = (gpu.dedicated_used, gpu.dedicated_total) {
            parts.push(i18n::of_total(
                &bytes(used as f64),
                &bytes(total as f64),
                self.locale,
            ));
        }
        if let Some(t) = gpu.temperature_celsius {
            parts.push(format!("{t:.0} °C"));
        }
        let detail = if parts.is_empty() {
            i18n::gpu_title(&gpu, self.cpu_model().as_deref(), self.locale)
        } else {
            parts.join(" · ")
        };
        self.resource_card(
            "card-gpu",
            c.gpu,
            IconName::Frame,
            None,
            gpu.utilization.map(share),
            Some((
                vec![ChartSeries::new(
                    self.live.values(&SeriesKey::Gpu(gpu.key.clone())),
                    ChartHue::Teal,
                )],
                100.0,
            )),
            detail,
            Resource::Gpu(gpu.key.clone()),
            cx,
        )
    }

    /// The processor's model, which on a machine with integrated graphics
    /// also names the graphics (`... w/ Radeon 890M`).
    fn cpu_model(&self) -> Option<String> {
        i18n::inventory_value(self.inventory.as_ref(), "cpu.model")
    }

    fn battery_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let Some(battery) = self.readings.system_battery().cloned() else {
            return self.resource_card(
                "card-battery",
                c.battery,
                IconName::Battery,
                None,
                None,
                None,
                if self.readings.has_battery {
                    c.no_battery.to_string()
                } else {
                    c.not_available.to_string()
                },
                Resource::Battery,
                cx,
            );
        };
        let p = palette(cx);
        self.resource_card(
            "card-battery",
            c.battery,
            IconName::Battery,
            Some((
                i18n::battery_status(battery.status.as_deref(), self.locale).to_string(),
                p.text_secondary,
            )),
            battery.charge.map(share),
            Some((
                vec![ChartSeries::new(
                    self.live
                        .values(&SeriesKey::BatteryCharge(battery.key.clone())),
                    ChartHue::Green,
                )],
                100.0,
            )),
            i18n::battery_line(&battery, self.locale),
            Resource::Battery,
            cx,
        )
    }

    // Processes.

    fn processes_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let (column, ascending) = self.app_sort;
        let lines = app_lines(&self.apps, column, ascending);
        // No program with a GPU reading, no GPU column; it is the last
        // column, so the others keep their positions.
        let has_gpu = crate::tables::shows_gpu_column(self.apps.processes());
        let rows: Vec<Vec<TableCell>> = lines
            .iter()
            .map(|line| {
                let mut cells = app_cells(&self.apps, *line, self.locale, self.processors());
                if !has_gpu {
                    cells.truncate(AppColumn::ALL.len() - 1);
                }
                cells
            })
            .collect();
        let selected = lines
            .iter()
            .position(|line| match (line, &self.app_selection) {
                (AppLine::Group { services, index }, Some(AppSelection::Group(key))) => {
                    app_row(&self.apps, *services, *index).is_some_and(|row| &row.group.key == key)
                }
                (AppLine::Member { process }, Some(AppSelection::Process(pid))) => self
                    .apps
                    .process(*process)
                    .is_some_and(|member| member.pid == *pid),
                _ => false,
            });
        let sort_index = AppColumn::ALL
            .iter()
            .position(|candidate| *candidate == column);
        let mut columns = vec![
            Column::new(c.column_name),
            Column::new(c.column_cpu).width(px(96.0)).align_end(),
            Column::new(c.column_memory).width(px(120.0)).align_end(),
            Column::new(c.column_disk).width(px(112.0)).align_end(),
            Column::new(c.column_network)
                .width(px(112.0))
                .align_end()
                .tooltip(c.network_column_tooltip),
        ];
        if has_gpu {
            columns.push(Column::new(c.column_gpu).width(px(72.0)).align_end());
        }
        let selection_of = {
            let lines = lines.clone();
            move |this: &MonitorApp, index: usize| -> Option<AppSelection> {
                match lines.get(index)? {
                    AppLine::Group { services, index } => app_row(&this.apps, *services, *index)
                        .map(|row| AppSelection::Group(row.group.key.clone())),
                    AppLine::Member { process } => this
                        .apps
                        .process(*process)
                        .map(|member| AppSelection::Process(member.pid)),
                    AppLine::Section { .. } => None,
                }
            }
        };
        let (select, invoke, menu) = (selection_of.clone(), selection_of.clone(), selection_of);
        DetailsTable::new("processes", columns, rows)
            .presorted()
            .fill()
            .sort(sort_index.map(|column| SortState { column, ascending }))
            .selected(selected)
            .empty(if self.rounds == 0 {
                c.waiting_for_first_reading
            } else {
                c.nothing_matches
            })
            .on_sort(Self::act_with(cx, |this, sort: SortState, _, cx| {
                if let Some(column) = AppColumn::ALL.get(sort.column) {
                    this.sort_apps(*column, cx);
                }
            }))
            .on_select(Self::act_with(cx, move |this, index: usize, _, _| {
                if let Some(selection) = select(this, index) {
                    this.app_selection = Some(selection);
                }
            }))
            .on_invoke(Self::act_with(
                cx,
                move |this, index: usize, _, cx| match invoke(this, index) {
                    Some(AppSelection::Group(key)) => this.toggle_expanded(&key),
                    Some(AppSelection::Process(pid)) => this.show_properties(pid, cx),
                    None => {}
                },
            ))
            .on_context_menu(Self::act_point(
                cx,
                move |this, index, position, window, cx| {
                    if let Some(selection) = menu(this, index) {
                        this.app_selection = Some(selection.clone());
                        this.open_menu(MenuTarget::App(selection), position, window, cx);
                    }
                },
            ))
            .into_any_element()
    }

    // Details.

    fn details_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let columns_shown = detail_columns(
            self.include_command_lines,
            crate::tables::shows_gpu_column(self.processes.processes()),
        );
        let (sorted, direction) = self.processes.sort();
        let tree = self.processes.tree_mode();
        let visible = self.processes.visible_rows().to_vec();
        let rows: Vec<Vec<TableCell>> = visible
            .iter()
            .filter_map(|row| {
                let process = self.processes.row(row.index)?;
                Some(
                    columns_shown
                        .iter()
                        .map(|column| {
                            let cell =
                                process_cell(process, *column, self.locale, self.processors());
                            if *column == ProcessColumn::Name && tree {
                                let cell = cell.indent(row.depth.min(8) as u8);
                                if row.has_children {
                                    cell.expander(true)
                                } else {
                                    cell
                                }
                            } else {
                                cell
                            }
                        })
                        .collect(),
                )
            })
            .collect();
        let pids: Vec<u32> = visible
            .iter()
            .filter_map(|row| self.processes.row(row.index).map(|process| process.pid))
            .collect();
        let selected = self
            .selected_pid
            .and_then(|pid| pids.iter().position(|candidate| *candidate == pid));
        let columns: Vec<Column> = columns_shown
            .iter()
            .map(|column| {
                let mut definition = Column::new(crate::tables::column_title(*column, c))
                    .sortable(!tree || *column != ProcessColumn::Name);
                if let Some(width) = detail_width(*column) {
                    definition = definition.width(px(width));
                }
                if numeric(*column) {
                    definition = definition.align_end();
                }
                if *column == ProcessColumn::State {
                    definition = definition.tooltip(c.state_column_tooltip);
                }
                if matches!(
                    *column,
                    ProcessColumn::ReadRate | ProcessColumn::WriteRate | ProcessColumn::Disk
                ) {
                    definition = definition.tooltip(c.disk_column_tooltip);
                }
                if *column == ProcessColumn::Network {
                    definition = definition.tooltip(c.network_column_tooltip);
                }
                definition
            })
            .collect();
        let sort = columns_shown
            .iter()
            .position(|column| *column == sorted)
            .map(|column| SortState {
                column,
                ascending: direction == SortDirection::Ascending,
            });
        let (select_pids, invoke_pids, menu_pids) = (pids.clone(), pids.clone(), pids);
        let sort_columns = columns_shown.clone();
        DetailsTable::new("details", columns, rows)
            .presorted()
            .fill()
            .sort(sort)
            .selected(selected)
            .empty(if self.rounds == 0 {
                c.waiting_for_first_reading
            } else {
                c.nothing_matches
            })
            .on_sort(Self::act_with(cx, move |this, sort: SortState, _, cx| {
                if let Some(column) = sort_columns.get(sort.column) {
                    this.sort_processes(*column, cx);
                }
            }))
            .on_select(Self::act_with(cx, move |this, index: usize, _, _| {
                this.selected_pid = select_pids.get(index).copied();
            }))
            .on_invoke(Self::act_with(cx, move |this, index: usize, _, cx| {
                if let Some(pid) = invoke_pids.get(index).copied() {
                    this.show_properties(pid, cx);
                }
            }))
            .on_context_menu(Self::act_point(
                cx,
                move |this, index, position, window, cx| {
                    if let Some(pid) = menu_pids.get(index).copied() {
                        this.selected_pid = Some(pid);
                        this.open_menu(MenuTarget::Process(pid), position, window, cx);
                    }
                },
            ))
            .into_any_element()
    }

    // Performance.

    fn performance_page(&mut self, available: f32, cx: &mut Context<Self>) -> AnyElement {
        let list_width = if available < 760.0 { 200.0 } else { 240.0 };
        let detail = match self.resource.clone() {
            Resource::Cpu => self.cpu_detail(available - list_width, cx),
            Resource::Memory => self.memory_detail(cx),
            Resource::Disk(name) => self.disk_detail(&name, cx),
            Resource::Adapter(name) => self.adapter_detail(&name, cx),
            Resource::Gpu(key) => self.gpu_detail(&key, cx),
            Resource::Npu(key) => self.npu_detail(&key, cx),
            Resource::Temperatures => self.temperature_detail(cx),
            Resource::Battery => self.battery_detail(cx),
        };
        div()
            .flex()
            .items_start()
            .gap(spacing::LG)
            .min_w_0()
            .child(
                div()
                    .w(px(list_width))
                    .flex_none()
                    .child(self.resource_list(cx)),
            )
            .child(div().flex_1().min_w_0().child(detail))
            .into_any_element()
    }

    /// The resources down the left of the Performance page, each with a
    /// small chart, as Task Manager lists them.
    fn resource_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let r = &self.readings;
        // The resource, its name, its figure, and its chart with the chart's top.
        type Tile = (Resource, String, String, Option<(Vec<ChartSeries>, f64)>);
        let mut tiles: Vec<Tile> = vec![
            (
                Resource::Cpu,
                c.cpu.to_string(),
                r.cpu
                    .utilization
                    .map(|u| match r.cpu.speed_hz {
                        Some(speed) => format!("{} {}", share(u), hertz(speed)),
                        None => share(u),
                    })
                    .unwrap_or_else(|| c.not_available.to_string()),
                Some((
                    vec![ChartSeries::new(
                        self.live.values(&SeriesKey::Cpu),
                        ChartHue::Blue,
                    )],
                    100.0,
                )),
            ),
            (
                Resource::Memory,
                c.memory.to_string(),
                match (
                    r.memory.in_use(),
                    r.memory.total,
                    r.memory.in_use_fraction(),
                ) {
                    (Some(used), Some(total), Some(fraction)) => format!(
                        "{}/{} ({})",
                        bytes(used as f64),
                        bytes(total as f64),
                        share(fraction)
                    ),
                    _ => c.not_available.to_string(),
                },
                Some((
                    vec![ChartSeries::new(
                        self.live.values(&SeriesKey::Memory),
                        ChartHue::Purple,
                    )],
                    r.memory.total.unwrap_or(1) as f64,
                )),
            ),
        ];
        for disk in &r.disks {
            tiles.push((
                Resource::Disk(disk.name.clone()),
                i18n::disk_title(&disk.name, disk.rotational, self.locale),
                disk.active
                    .map(share)
                    .unwrap_or_else(|| c.not_available.to_string()),
                Some((
                    vec![ChartSeries::new(
                        self.live.values(&SeriesKey::DiskActive(disk.name.clone())),
                        ChartHue::Green,
                    )],
                    100.0,
                )),
            ));
        }
        for adapter in &r.adapters {
            let series = vec![
                ChartSeries::new(
                    self.live.values(&SeriesKey::Receive(adapter.name.clone())),
                    ChartHue::Orange,
                ),
                ChartSeries::new(
                    self.live.values(&SeriesKey::Send(adapter.name.clone())),
                    ChartHue::Orange,
                ),
            ];
            let top = byte_axis(peak(&series).unwrap_or(0.0), 1024.0);
            tiles.push((
                Resource::Adapter(adapter.name.clone()),
                i18n::adapter_title(adapter.kind, &adapter.name, self.locale),
                if adapter.connected == Some(false) {
                    i18n::adapter_status(Some(false), self.locale).to_string()
                } else {
                    i18n::receive_send_short(
                        adapter.receive_rate.map(rate),
                        adapter.send_rate.map(rate),
                        self.locale,
                    )
                },
                Some((series, top)),
            ));
        }
        for gpu in &r.gpus {
            tiles.push((
                Resource::Gpu(gpu.key.clone()),
                if r.gpus.len() > 1 {
                    format!("{} {}", c.gpu, gpu.card.clone().unwrap_or_default())
                } else {
                    c.gpu.to_string()
                },
                match (gpu.utilization, gpu.temperature_celsius) {
                    // A powered-down card is not read, so as not to wake it.
                    _ if gpu.source.as_deref() == Some("asleep") => c.npu_asleep.to_string(),
                    (Some(u), Some(t)) => format!("{} ({t:.0} °C)", share(u)),
                    (Some(u), None) => share(u),
                    _ => c.not_available.to_string(),
                },
                Some((
                    vec![ChartSeries::new(
                        self.live.values(&SeriesKey::Gpu(gpu.key.clone())),
                        ChartHue::Teal,
                    )],
                    100.0,
                )),
            ));
        }
        for npu in &r.npus {
            tiles.push((
                Resource::Npu(npu.key.clone()),
                c.npu.to_string(),
                match (npu.utilization, i18n::npu_state(npu, self.locale)) {
                    (Some(u), Some(state)) => format!("{} · {state}", share(u)),
                    (Some(u), None) => share(u),
                    (None, Some(state)) => state.to_string(),
                    (None, None) => c.not_available.to_string(),
                },
                Some((
                    vec![ChartSeries::new(
                        self.live.values(&SeriesKey::Npu(npu.key.clone())),
                        ChartHue::Magenta,
                    )],
                    100.0,
                )),
            ));
        }
        if !r.sensors.is_empty() || r.cpu.temperature_celsius.is_some() {
            let hottest = r.hottest();
            let fan = r.fastest_fan();
            let mut value = hottest
                .and_then(|sensor| sensor.value)
                .or(r.cpu.temperature_celsius)
                .map(|t| format!("{t:.0} °C"))
                .unwrap_or_else(|| c.not_available.to_string());
            if let Some(rpm) = fan {
                value = format!("{value} · {}", i18n::sensor_value("fan", rpm));
            }
            let series_key = hottest
                .map(|sensor| SeriesKey::Sensor(sensor.key.clone()))
                .unwrap_or(SeriesKey::CpuTemperature);
            tiles.push((
                Resource::Temperatures,
                c.temperatures.to_string(),
                value,
                Some((
                    vec![ChartSeries::new(
                        self.live.values(&series_key),
                        ChartHue::Red,
                    )],
                    100.0,
                )),
            ));
        }
        if let Some(battery) = r.system_battery() {
            tiles.push((
                Resource::Battery,
                c.battery.to_string(),
                match battery.charge {
                    Some(charge) => format!(
                        "{} · {}",
                        share(charge),
                        i18n::battery_status(battery.status.as_deref(), self.locale)
                    ),
                    None => c.not_available.to_string(),
                },
                Some((
                    vec![ChartSeries::new(
                        self.live
                            .values(&SeriesKey::BatteryCharge(battery.key.clone())),
                        ChartHue::Green,
                    )],
                    100.0,
                )),
            ));
        }

        // Rows 2px apart, the 2px inside each row's hit area (ticket 103), so
        // a press between two tiles still lands on one.
        let mut list = div().flex().flex_col();
        for (index, (resource, title, value, chart)) in tiles.into_iter().enumerate() {
            let selected = resource == self.resource;
            let fill = better_ui::fluent::list::row_fill(selected, Interaction::Rest, p);
            let hover = better_ui::fluent::list::row_fill(selected, Interaction::PointerOver, p);
            let pressed = better_ui::fluent::list::row_fill(selected, Interaction::Pressed, p);
            let target = resource.clone();
            let thumbnail = match chart {
                Some((series, top)) => div()
                    .w(px(64.0))
                    .h(px(40.0))
                    .flex_none()
                    .rounded(radius::SM)
                    .border_1()
                    .border_color(p.card_stroke)
                    .child(
                        LineChart::sparkline(
                            ElementId::named_usize("tile-chart", index),
                            series,
                            CAPACITY,
                            top,
                        )
                        .height(px(38.0)),
                    ),
                None => div()
                    .w(px(64.0))
                    .h(px(40.0))
                    .flex_none()
                    .rounded(radius::SM)
                    .border_1()
                    .border_color(p.card_stroke),
            };
            let look = div()
                .id("look")
                .relative()
                .flex()
                .items_center()
                .gap(spacing::MD)
                .px(spacing::MD)
                .py(spacing::SM)
                .rounded(radius::SM)
                .bg(fill);
            let mut tile = better_ui::fluent::hit_area::group_look(look, hover, pressed)
                .child(thumbnail)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(label(title, BODY, p.text_primary).truncate())
                        .child(label(value, CAPTION, p.text_secondary).truncate()),
                );
            if selected {
                tile = tile.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(better_ui::fluent::list::selection_bar(p, false)),
                );
            }
            list = list.child(
                better_ui::fluent::hit_area::hit_row(
                    ElementId::named_usize("resource-tile", index),
                    better_ui::fluent::hit_area::RowInset::LIST,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.resource = target.clone();
                    cx.notify();
                }))
                .child(tile),
            );
        }
        list.into_any_element()
    }

    fn detail_header(&self, title: String, subtitle: Option<String>, p: &Palette) -> AnyElement {
        div()
            .flex()
            .items_end()
            .justify_between()
            .gap(spacing::MD)
            .min_w_0()
            .child(label(title, SUBTITLE, p.text_primary).min_w_0().truncate())
            .children(
                subtitle
                    .map(|subtitle| label(subtitle, BODY, p.text_secondary).min_w_0().truncate()),
            )
            .into_any_element()
    }

    fn span_label(&self) -> &'static str {
        copy(self.locale).sixty_seconds
    }

    fn cpu_detail(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        if !self.readings.has_cpu {
            return note_card(c.cpu, c.not_reported_yet, cx);
        }
        let cpu = self.readings.cpu.clone();
        let model = i18n::inventory_value(self.inventory.as_ref(), "cpu.model");
        let chart = if self.per_core && !cpu.cores.is_empty() {
            let columns = ((width / 150.0).floor() as usize).clamp(2, 8);
            let charts: Vec<AnyElement> = cpu
                .cores
                .iter()
                .map(|core| {
                    LineChart::new(
                        ElementId::named_usize("core-chart", core.index as usize),
                        vec![ChartSeries::new(
                            self.live.values(&SeriesKey::Core(core.index)),
                            ChartHue::Blue,
                        )],
                        CAPACITY,
                        100.0,
                    )
                    .height(px(64.0))
                    .into_any_element()
                })
                .collect();
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .child(label(
                            c.utilization_per_processor,
                            CAPTION,
                            p.text_secondary,
                        ))
                        .child(label("100%", CAPTION, p.text_secondary)),
                )
                .child(grid_tight(columns, charts))
                .into_any_element()
        } else {
            LineChart::new(
                "cpu-chart",
                vec![ChartSeries::new(
                    self.live.values(&SeriesKey::Cpu),
                    ChartHue::Blue,
                )],
                CAPACITY,
                100.0,
            )
            .height(px(240.0))
            .caption(c.utilization_percent)
            .maximum_label("100%")
            .span_label(self.span_label())
            .minimum_label("0")
            .into_any_element()
        };
        let view = SelectorBar::new(
            "cpu-view",
            vec![
                SelectorItem::new(c.overall_utilization),
                SelectorItem::new(c.logical_processors),
            ],
            Some(if self.per_core { 1 } else { 0 }),
        )
        .on_select(Self::act_with(cx, |this, index: usize, _, _| {
            this.per_core = index == 1
        }));
        let processes = self.overview.process_count.value().copied();
        let stats = div()
            .flex()
            .flex_wrap()
            .gap(spacing::XL)
            .child(stat_or_missing(
                c.utilization,
                cpu.utilization.map(share),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.speed,
                cpu.speed_hz.map(hertz),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.processes_count,
                processes.map(|n| n.to_string()),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.threads,
                cpu.threads.map(|n| n.to_string()),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.up_time,
                cpu.uptime_seconds
                    .map(|s| i18n::uptime(s as u64, self.locale)),
                true,
                c,
                p,
            ));
        let facts = card(p)
            .gap(px(0.0))
            .child(crate::shell::property_row(
                c.maximum_speed,
                cpu.maximum_hz
                    .map(hertz)
                    .unwrap_or_else(|| c.not_available.to_string()),
                cpu.maximum_hz.is_none(),
                p,
            ))
            .child(crate::shell::property_row(
                c.logical_processors,
                cpu.logical
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| c.not_available.to_string()),
                cpu.logical.is_none(),
                p,
            ))
            .child(crate::shell::property_row(
                c.waiting_to_run,
                cpu.load_1m
                    .map(|l| format!("{l:.2}"))
                    .unwrap_or_else(|| c.not_available.to_string()),
                cpu.load_1m.is_none(),
                p,
            ))
            .child(crate::shell::property_row(
                c.temperature,
                cpu.temperature_celsius
                    .map(|t| format!("{t:.0} °C"))
                    .unwrap_or_else(|| c.not_read.to_string()),
                cpu.temperature_celsius.is_none(),
                p,
            ))
            .child(div().pt(spacing::SM).child(label(
                c.waiting_to_run_hint,
                CAPTION,
                p.text_secondary,
            )));
        div()
            .flex()
            .flex_col()
            .gap(spacing::LG)
            .child(self.detail_header(c.cpu.to_string(), model, p))
            .child(view)
            .child(card(p).child(chart))
            .child(stats)
            .child(facts)
            .into_any_element()
    }

    fn memory_detail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        if !self.readings.has_memory {
            return note_card(c.memory, c.not_reported_yet, cx);
        }
        let m = self.readings.memory.clone();
        let total = m.total.unwrap_or(0) as f64;
        let part = |value: Option<u64>| -> f32 {
            match value {
                Some(value) if total > 0.0 => (value as f64 / total) as f32,
                _ => 0.0,
            }
        };
        let in_use = m.in_use();
        let modified = m.modified;
        // Changes waiting to be saved are part of the file cache; the bar
        // shows them once, beside the rest of it.
        let cached = m.cached.map(|c| c.saturating_sub(m.modified.unwrap_or(0)));
        let free = m.free;
        let hue = ChartHue::Purple.color(p.is_dark());
        let composition = CompositionBar::new(vec![
            (part(in_use), with_alpha(hue, 0.55)),
            (part(modified), with_alpha(hue, 0.85)),
            (part(cached), with_alpha(hue, 0.25)),
            (part(free), with_alpha(hue, 0.0)),
        ]);
        let legend = div()
            .flex()
            .flex_wrap()
            .gap(spacing::LG)
            .child(legend_item(c.in_use, with_alpha(hue, 0.55), in_use, c, p))
            .child(legend_item(
                c.modified,
                with_alpha(hue, 0.85),
                modified,
                c,
                p,
            ))
            .child(legend_item(c.cached, with_alpha(hue, 0.25), cached, c, p))
            .child(legend_item(c.free, p.card_stroke, free, c, p));
        let chart = LineChart::new(
            "memory-chart",
            vec![ChartSeries::new(
                self.live.values(&SeriesKey::Memory),
                ChartHue::Purple,
            )],
            CAPACITY,
            total.max(1.0),
        )
        .height(px(200.0))
        .caption(c.memory_usage)
        .maximum_label(bytes(total))
        .span_label(self.span_label())
        .minimum_label("0");
        let optional = |value: Option<u64>| value.map(|v| bytes(v as f64));
        let stats = div()
            .flex()
            .flex_wrap()
            .gap(spacing::XL)
            .child(stat_or_missing(c.in_use, optional(in_use), true, c, p))
            .child(stat_or_missing(
                c.available,
                optional(m.available),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.committed,
                match (m.committed, m.commit_limit) {
                    (Some(a), Some(b)) => Some(format!("{}/{}", bytes(a as f64), bytes(b as f64))),
                    (a, _) => optional(a),
                },
                true,
                c,
                p,
            ))
            .child(stat_or_missing(c.cached, optional(cached), true, c, p))
            .child(stat_or_missing(
                c.swap,
                match (m.swap_used, m.swap_total) {
                    (Some(_), Some(0)) => Some(c.swap_off.to_string()),
                    (Some(a), Some(b)) => Some(format!("{}/{}", bytes(a as f64), bytes(b as f64))),
                    _ => None,
                },
                true,
                c,
                p,
            ));
        div()
            .flex()
            .flex_col()
            .gap(spacing::LG)
            .child(self.detail_header(c.memory.to_string(), optional(m.total), p))
            .child(card(p).child(chart))
            .child(
                card(p)
                    .child(label(c.memory_composition, BODY_STRONG, p.text_primary))
                    .child(composition)
                    .child(legend),
            )
            .child(stats)
            .child(label(c.memory_hint, CAPTION, p.text_secondary))
            .into_any_element()
    }

    fn disk_detail(&mut self, name: &str, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let Some(disk) = self.readings.disks.iter().find(|d| d.name == name).cloned() else {
            return note_card(c.disk, c.not_reported_yet, cx);
        };
        let active = LineChart::new(
            "disk-active",
            vec![ChartSeries::new(
                self.live.values(&SeriesKey::DiskActive(disk.name.clone())),
                ChartHue::Green,
            )],
            CAPACITY,
            100.0,
        )
        .height(px(160.0))
        .caption(c.active_time)
        .maximum_label("100%")
        .span_label(self.span_label())
        .minimum_label("0");
        let series = vec![
            ChartSeries::new(
                self.live.values(&SeriesKey::DiskRead(disk.name.clone())),
                ChartHue::Green,
            ),
            ChartSeries::new(
                self.live.values(&SeriesKey::DiskWrite(disk.name.clone())),
                ChartHue::Green,
            ),
        ];
        let top = byte_axis(peak(&series).unwrap_or(0.0), 1024.0 * 1024.0);
        let transfer = LineChart::new("disk-transfer", series, CAPACITY, top)
            .height(px(120.0))
            .caption(c.transfer_rate)
            .maximum_label(rate(top))
            .span_label(c.read_solid_write_dashed)
            .minimum_label("0");
        let stats = div()
            .flex()
            .flex_wrap()
            .gap(spacing::XL)
            .child(stat_or_missing(
                c.active_time_short,
                disk.active.map(share),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.response_time,
                disk.response_ms().map(|ms| format!("{ms:.1} ms")),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.read_speed,
                disk.read_rate.map(rate),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.write_speed,
                disk.write_rate.map(rate),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.capacity,
                disk.capacity.map(|b| bytes(b as f64)),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.kind,
                disk.rotational
                    .map(|spinning| if spinning { c.hdd } else { c.ssd }.to_string()),
                true,
                c,
                p,
            ));
        div()
            .flex()
            .flex_col()
            .gap(spacing::LG)
            .child(self.detail_header(
                i18n::disk_title(&disk.name, disk.rotational, self.locale),
                disk.model.clone(),
                p,
            ))
            .child(card(p).child(active).child(transfer))
            .child(stats)
            .into_any_element()
    }

    fn adapter_detail(&mut self, name: &str, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let Some(adapter) = self
            .readings
            .adapters
            .iter()
            .find(|a| a.name == name)
            .cloned()
        else {
            return note_card(c.network, c.not_reported_yet, cx);
        };
        let series = vec![
            ChartSeries::new(
                self.live.values(&SeriesKey::Receive(adapter.name.clone())),
                ChartHue::Orange,
            ),
            ChartSeries::new(
                self.live.values(&SeriesKey::Send(adapter.name.clone())),
                ChartHue::Orange,
            ),
        ];
        let top = byte_axis(peak(&series).unwrap_or(0.0), 1024.0);
        let chart = LineChart::new("adapter-chart", series, CAPACITY, top)
            .height(px(220.0))
            .caption(c.throughput)
            .maximum_label(rate(top))
            .span_label(c.receive_solid_send_dashed)
            .minimum_label("0");
        let stats = div()
            .flex()
            .flex_wrap()
            .gap(spacing::XL)
            .child(stat_or_missing(
                c.send,
                adapter.send_rate.map(rate),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.receive,
                adapter.receive_rate.map(rate),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.connection,
                Some(i18n::adapter_status(adapter.connected, self.locale).to_string()),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.link_speed,
                adapter.link_speed_bps.map(i18n::bits_per_second),
                true,
                c,
                p,
            ));
        let kind = match adapter.kind {
            AdapterKind::Wireless => c.wireless,
            AdapterKind::Wired => c.wired,
            AdapterKind::Other => c.other_connection,
        };
        div()
            .flex()
            .flex_col()
            .gap(spacing::LG)
            .child(self.detail_header(
                i18n::adapter_title(adapter.kind, &adapter.name, self.locale),
                Some(format!("{kind} · {}", adapter.name)),
                p,
            ))
            .child(card(p).child(chart))
            .child(stats)
            .into_any_element()
    }

    fn gpu_detail(&mut self, key: &str, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let Some(gpu) = self.readings.gpus.iter().find(|g| g.key == key).cloned() else {
            return note_card(c.gpu, c.gpu_not_found, cx);
        };
        let busy = ChartSeries::new(
            self.live.values(&SeriesKey::Gpu(gpu.key.clone())),
            ChartHue::Teal,
        );
        let chart = card(p).child(
            LineChart::new("gpu-chart", vec![busy], CAPACITY, 100.0)
                .height(px(200.0))
                .caption(c.utilization_percent)
                .maximum_label("100%")
                .span_label(self.span_label())
                .minimum_label("0"),
        );
        let memory_chart = gpu.dedicated_total.map(|total| {
            card(p).child(
                LineChart::new(
                    "gpu-memory-chart",
                    vec![ChartSeries::new(
                        self.live.values(&SeriesKey::GpuMemory(gpu.key.clone())),
                        ChartHue::Purple,
                    )],
                    CAPACITY,
                    (total as f64).max(1.0),
                )
                .height(px(120.0))
                .caption(c.dedicated_memory)
                .maximum_label(bytes(total as f64))
                .span_label(self.span_label())
                .minimum_label("0"),
            )
        });
        let pair = |used: Option<u64>, total: Option<u64>| match (used, total) {
            (Some(used), Some(total)) => Some(i18n::of_total(
                &bytes(used as f64),
                &bytes(total as f64),
                self.locale,
            )),
            (Some(used), None) => Some(bytes(used as f64)),
            _ => None,
        };
        let stats = div()
            .flex()
            .flex_wrap()
            .gap(spacing::XL)
            .child(stat_or_missing(
                c.utilization,
                gpu.utilization.map(share),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.dedicated_memory,
                pair(gpu.dedicated_used, gpu.dedicated_total),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.shared_memory,
                pair(gpu.shared_used, gpu.shared_total),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.temperature,
                gpu.temperature_celsius.map(|t| format!("{t:.0} °C")),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.power,
                gpu.power_watts.map(crate::tables::watts),
                true,
                c,
                p,
            ));
        let missing = |value: Option<String>| {
            let none = value.is_none();
            (value.unwrap_or_else(|| c.not_available.to_string()), none)
        };
        let mut facts = card(p).gap(px(0.0));
        for (name, (value, none)) in [
            (
                c.gpu_clock,
                missing(
                    gpu.frequency_hz
                        .map(|hz| i18n::sensor_value("frequency", hz)),
                ),
            ),
            (
                c.memory_clock,
                missing(
                    gpu.memory_frequency_hz
                        .map(|hz| i18n::sensor_value("frequency", hz)),
                ),
            ),
            (c.driver, missing(gpu.driver.clone())),
            (c.location, missing(Some(format!("PCI {}", gpu.key)))),
            (
                c.programs_using,
                missing(gpu.clients.map(|n| n.to_string())),
            ),
        ] {
            facts = facts.child(crate::shell::property_row(name, value, none, p));
        }
        if gpu.source.as_deref() == Some("clients") {
            facts = facts.child(div().pt(spacing::SM).child(label(
                c.busy_from_clients,
                CAPTION,
                p.text_secondary,
            )));
        }
        let mut page = div()
            .flex()
            .flex_col()
            .gap(spacing::LG)
            .child(self.detail_header(
                c.gpu.to_string(),
                Some(i18n::gpu_title(
                    &gpu,
                    self.cpu_model().as_deref(),
                    self.locale,
                )),
                p,
            ));
        if gpu.utilization.is_none() && self.rounds > 2 {
            page = page.child(
                InfoBar::new(
                    "gpu-unreported",
                    Severity::Informational,
                    c.gpu_busy_unreported,
                )
                .locale(self.locale.words()),
            );
        }
        page = page
            .child(chart)
            .children(memory_chart)
            .child(stats)
            .child(facts);
        page = page
            .child(section_title(c.gpu_apps, p))
            .child(self.top_apps_table(
                "gpu-apps",
                |row| row.gpu_utilization.total,
                |row, _| share(row.gpu_utilization.total.min(1.0)),
                c.column_gpu,
                cx,
            ));
        page.into_any_element()
    }

    /// The five applications using the most of one thing, by an aggregate of
    /// their processes, as a small table.
    fn top_apps_table(
        &self,
        id: &'static str,
        score: impl Fn(&monitor_views::apps::AppRow) -> f64,
        figure: impl Fn(&monitor_views::apps::AppRow, f64) -> String,
        column: &'static str,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let mut rows: Vec<&monitor_views::apps::AppRow> = self
            .apps
            .applications()
            .iter()
            .chain(self.apps.services().iter())
            .filter(|row| score(row) > 0.0)
            .collect();
        rows.sort_by(|a, b| score(b).total_cmp(&score(a)));
        rows.truncate(5);
        let cells: Vec<Vec<TableCell>> = rows
            .iter()
            .map(|row| {
                vec![
                    TableCell::new(row.group.display_name.clone()),
                    TableCell::new(figure(row, score(row))),
                ]
            })
            .collect();
        DetailsTable::new(
            id,
            vec![
                Column::new(c.column_name),
                Column::new(column).width(px(120.0)).align_end(),
            ],
            cells,
        )
        .empty(c.battery_use_nothing)
        .into_any_element()
    }

    fn npu_detail(&mut self, key: &str, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let Some(npu) = self.readings.npus.iter().find(|n| n.key == key).cloned() else {
            return note_card(c.npu, c.not_reported_yet, cx);
        };
        let chart = card(p).child(
            LineChart::new(
                "npu-chart",
                vec![ChartSeries::new(
                    self.live.values(&SeriesKey::Npu(npu.key.clone())),
                    ChartHue::Magenta,
                )],
                CAPACITY,
                100.0,
            )
            .height(px(200.0))
            .caption(c.utilization_percent)
            .maximum_label("100%")
            .span_label(self.span_label())
            .minimum_label("0"),
        );
        let state = i18n::npu_state(&npu, self.locale).map(str::to_string);
        let stats = div()
            .flex()
            .flex_wrap()
            .gap(spacing::XL)
            .child(stat_or_missing(
                c.utilization,
                npu.utilization.map(share),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(c.power_state, state, true, c, p))
            .child(stat_or_missing(
                c.awake_time,
                npu.awake.map(share),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.programs_using,
                npu.clients.map(|n| n.to_string()),
                true,
                c,
                p,
            ));
        let missing = |value: Option<String>| {
            let none = value.is_none();
            (value.unwrap_or_else(|| c.not_available.to_string()), none)
        };
        let mut facts = card(p).gap(px(0.0));
        for (name, (value, none)) in [
            (c.battery_model, missing(npu.model.clone())),
            (c.firmware, missing(npu.firmware.clone())),
            (c.driver, missing(npu.driver.clone())),
            (
                c.power_state,
                missing(match (&npu.power_state, &npu.runtime_status) {
                    (Some(state), Some(runtime)) => Some(format!("{state} · {runtime}")),
                    (one, other) => one.clone().or(other.clone()),
                }),
            ),
            (c.location, missing(Some(format!("PCI {}", npu.key)))),
        ] {
            facts = facts.child(crate::shell::property_row(name, value, none, p));
        }
        let note = if npu.utilization_unreported {
            Some(c.npu_no_busy)
        } else if npu.clients == Some(0) {
            Some(c.npu_idle)
        } else {
            None
        };
        div()
            .flex()
            .flex_col()
            .gap(spacing::LG)
            .child(self.detail_header(
                c.npu.to_string(),
                Some(i18n::npu_title(&npu, self.locale)),
                p,
            ))
            .children(note.map(|note| {
                InfoBar::new("npu-note", Severity::Informational, note).locale(self.locale.words())
            }))
            .child(chart)
            .child(stats)
            .child(facts)
            .into_any_element()
    }

    fn temperature_detail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let r = self.readings.clone();
        if r.sensors.is_empty() {
            let temperature = r.cpu.temperature_celsius;
            return div()
                .flex()
                .flex_col()
                .gap(spacing::LG)
                .child(self.detail_header(c.sensors.to_string(), None, p))
                .child(match temperature {
                    Some(t) => {
                        crate::shell::stat(c.processor_temperature, format!("{t:.0} °C"), true, p)
                            .into_any_element()
                    }
                    None => note_card(c.sensors, c.no_sensors, cx),
                })
                .into_any_element();
        }
        // The chart: every temperature together, hottest first.
        let mut temperatures: Vec<&crate::charts::SensorReadings> = r
            .sensors_of("temperature")
            .filter(|s| s.value.is_some())
            .collect();
        temperatures.sort_by(|a, b| b.value.unwrap_or(0.0).total_cmp(&a.value.unwrap_or(0.0)));
        const HUES: [ChartHue; 6] = [
            ChartHue::Red,
            ChartHue::Orange,
            ChartHue::Blue,
            ChartHue::Teal,
            ChartHue::Purple,
            ChartHue::Green,
        ];
        let series: Vec<ChartSeries> = temperatures
            .iter()
            .take(6)
            .enumerate()
            .map(|(index, sensor)| {
                ChartSeries::new(
                    self.live.values(&SeriesKey::Sensor(sensor.key.clone())),
                    HUES[index % HUES.len()],
                )
            })
            .collect();
        let top = nice_maximum(peak(&series).unwrap_or(0.0), 100.0);
        let legend = div().flex().flex_wrap().gap(spacing::LG).children(
            temperatures
                .iter()
                .take(6)
                .enumerate()
                .map(|(index, sensor)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .size(px(10.0))
                                .rounded_full()
                                .bg(HUES[index % HUES.len()].color(p.is_dark())),
                        )
                        .child(label(
                            i18n::sensor_title(sensor, self.locale),
                            CAPTION,
                            p.text_secondary,
                        ))
                        .into_any_element()
                }),
        );
        let chart = card(p)
            .gap(spacing::SM)
            .child(
                LineChart::new("temperature-chart", series, CAPACITY, top)
                    .height(px(200.0))
                    .caption(c.temperatures_heading)
                    .maximum_label(format!("{top:.0} °C"))
                    .span_label(self.span_label())
                    .minimum_label("0 °C"),
            )
            .child(legend);

        let section = |title: &'static str, kind: &str| -> Option<AnyElement> {
            let rows: Vec<&crate::charts::SensorReadings> = r.sensors_of(kind).collect();
            if rows.is_empty() {
                return None;
            }
            let mut list = card(p).gap(px(0.0));
            for sensor in rows {
                let mut value = sensor
                    .value
                    .map(|v| i18n::sensor_value(kind, v))
                    .unwrap_or_else(|| c.not_available.to_string());
                if let Some(critical) = sensor.critical {
                    value = format!("{value}  ({} {critical:.0} °C)", c.critical_at);
                }
                list = list.child(crate::shell::property_row(
                    i18n::sensor_title(sensor, self.locale),
                    value,
                    sensor.value.is_none(),
                    p,
                ));
            }
            Some(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::SM)
                    .child(section_title(title, p))
                    .child(list)
                    .into_any_element(),
            )
        };
        let mut page = div()
            .flex()
            .flex_col()
            .gap(spacing::LG)
            .child(self.detail_header(c.sensors.to_string(), None, p))
            .child(chart)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(spacing::XL)
                    .child(stat_or_missing(
                        c.hottest,
                        r.hottest()
                            .and_then(|s| s.value)
                            .map(|t| format!("{t:.0} °C")),
                        true,
                        c,
                        p,
                    ))
                    .child(stat_or_missing(
                        c.fan_speed,
                        r.fastest_fan().map(|rpm| i18n::sensor_value("fan", rpm)),
                        true,
                        c,
                        p,
                    )),
            );
        for (title, kind) in [
            (c.temperatures_heading, "temperature"),
            (c.fans, "fan"),
            (c.power_readings, "power"),
            (c.voltages, "voltage"),
            (c.currents, "current"),
            (c.clocks, "frequency"),
        ] {
            page = page.children(section(title, kind));
        }
        if r.sensors_of("fan").next().is_none() {
            page = page.child(label(c.no_fans, BODY, p.text_secondary));
        }
        page.into_any_element()
    }

    fn battery_detail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let Some(battery) = self.readings.system_battery().cloned() else {
            return note_card(c.battery, c.no_battery, cx);
        };
        let chart = card(p).child(
            LineChart::new(
                "battery-chart",
                vec![ChartSeries::new(
                    self.live
                        .values(&SeriesKey::BatteryCharge(battery.key.clone())),
                    ChartHue::Green,
                )],
                CAPACITY,
                100.0,
            )
            .height(px(160.0))
            .caption(c.battery_charge_chart)
            .maximum_label("100%")
            .span_label(self.span_label())
            .minimum_label("0"),
        );
        let charging = battery.status.as_deref() == Some("charging");
        let stats = div()
            .flex()
            .flex_wrap()
            .gap(spacing::XL)
            .child(stat_or_missing(
                c.charge,
                battery.charge.map(share),
                true,
                c,
                p,
            ))
            .child(crate::shell::stat(
                c.column_state,
                i18n::battery_status(battery.status.as_deref(), self.locale).to_string(),
                true,
                p,
            ))
            .child(stat_or_missing(
                if charging {
                    c.time_to_full
                } else {
                    c.time_left
                },
                if charging {
                    battery.time_to_full
                } else {
                    battery.time_remaining
                }
                .map(|s| i18n::hours_minutes(s, self.locale)),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                if charging { c.charging_at } else { c.drawing },
                battery.power_watts.map(crate::tables::watts),
                true,
                c,
                p,
            ))
            .child(stat_or_missing(
                c.battery_health,
                battery.health.map(share),
                true,
                c,
                p,
            ));
        let missing = |value: Option<String>| {
            let none = value.is_none();
            (value.unwrap_or_else(|| c.not_available.to_string()), none)
        };
        let mut facts = card(p).gap(px(0.0));
        let model = match (&battery.manufacturer, &battery.model) {
            (Some(m), Some(model)) => Some(format!("{m} {model}")),
            (one, other) => one.clone().or(other.clone()),
        };
        for (name, (value, none)) in [
            (
                c.full_capacity,
                missing(battery.energy_full.map(i18n::watt_hours)),
            ),
            (
                c.design_capacity,
                missing(battery.energy_design.map(i18n::watt_hours)),
            ),
            (
                c.charge_cycles,
                match battery.cycles {
                    Some(n) => (n.to_string(), false),
                    None => (c.not_reported_by_firmware.to_string(), true),
                },
            ),
            (
                c.voltage,
                missing(battery.voltage.map(|v| i18n::sensor_value("voltage", v))),
            ),
            (c.charge_limit, missing(battery.charge_limit.map(share))),
            (c.battery_model, missing(model)),
            (c.kind, missing(battery.technology.clone())),
        ] {
            facts = facts.child(crate::shell::property_row(name, value, none, p));
        }
        let mut chargers = card(p).gap(px(0.0));
        let mut any_charger = false;
        for source in &self.readings.power_sources {
            any_charger = true;
            let state = match source.online {
                Some(true) => match source.max_power {
                    Some(w) => format!("{} · {}", c.connected, crate::tables::watts(w)),
                    None => c.connected.to_string(),
                },
                Some(false) => c.not_connected.to_string(),
                None => c.not_available.to_string(),
            };
            chargers = chargers.child(crate::shell::property_row(
                i18n::power_source_title(source, self.locale),
                state,
                source.online != Some(true),
                p,
            ));
        }
        let mut page = div()
            .flex()
            .flex_col()
            .gap(spacing::LG)
            .child(self.detail_header(
                c.battery.to_string(),
                Some(i18n::battery_line(&battery, self.locale)),
                p,
            ))
            .child(chart)
            .child(stats)
            .child(facts);
        if any_charger {
            page = page.child(section_title(c.chargers, p)).child(chargers);
        }
        page.child(self.battery_use_section(cx)).into_any_element()
    }

    /// "What is using the battery": now, from this round's estimate, and since
    /// the machine was unplugged, from the service's running total.
    fn battery_use_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let on_battery = self.readings.on_battery == Some(true);
        let now: AnyElement = if on_battery {
            self.top_apps_table(
                "battery-now",
                |row| row.energy_power.total,
                |_, watts| crate::tables::watts(watts),
                c.column_power,
                cx,
            )
        } else {
            label(c.battery_use_plugged, BODY, p.text_secondary).into_any_element()
        };
        let since: AnyElement = match self.status.as_ref().and_then(|s| s.battery_use.clone()) {
            None => label(c.battery_use_never, BODY, p.text_secondary).into_any_element(),
            Some(use_) => {
                let rows: Vec<Vec<TableCell>> = use_
                    .processes
                    .iter()
                    .take(8)
                    .map(|process| {
                        let share_of = if use_.total_watt_hours > 0.0 {
                            format!(
                                " ({})",
                                share((process.watt_hours / use_.total_watt_hours).min(1.0))
                            )
                        } else {
                            String::new()
                        };
                        vec![
                            TableCell::new(process.name.clone()),
                            TableCell::new(format!(
                                "{}{share_of}",
                                i18n::watt_hours(process.watt_hours)
                            )),
                        ]
                    })
                    .collect();
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::SM)
                    .child(label(
                        i18n::since_unplugged(
                            use_.on_battery_seconds,
                            use_.total_watt_hours,
                            self.locale,
                        ),
                        CAPTION,
                        p.text_secondary,
                    ))
                    .child(
                        DetailsTable::new(
                            "battery-since",
                            vec![
                                Column::new(c.column_name),
                                Column::new(c.column_energy).width(px(160.0)).align_end(),
                            ],
                            rows,
                        )
                        .empty(c.battery_use_nothing),
                    )
                    .into_any_element()
            }
        };
        div()
            .flex()
            .flex_col()
            .gap(spacing::MD)
            .child(section_title(c.battery_use_title, p))
            .child(label(c.battery_use_note, CAPTION, p.text_secondary))
            .child(label(c.battery_use_now, BODY_STRONG, p.text_primary))
            .child(now)
            .child(label(c.battery_use_since, BODY_STRONG, p.text_primary))
            .child(since)
            .into_any_element()
    }

    // Startup apps.

    fn startup_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let entries = self.startup.entries.clone();
        let rows: Vec<Vec<TableCell>> = entries
            .iter()
            .map(|entry| {
                let mut name = TableCell::new(entry.name.clone());
                if let Some(comment) = &entry.comment {
                    name = name.tooltip(comment.clone());
                }
                let status = if !entry.installed {
                    TableCell::new(c.not_installed)
                        .tone(CellTone::Secondary)
                        .tooltip(c.not_installed_hint)
                } else if !entry.for_this_desktop {
                    TableCell::new(c.not_on_this_desktop).tone(CellTone::Secondary)
                } else if entry.enabled {
                    TableCell::new(c.enabled)
                } else {
                    TableCell::new(c.disabled).tone(CellTone::Secondary)
                };
                vec![
                    name,
                    status,
                    TableCell::new(i18n::scope_word(entry.scope, self.locale)),
                    TableCell::new(entry.command.clone().unwrap_or_default())
                        .tone(CellTone::Secondary),
                ]
            })
            .collect();
        let selected = self
            .startup
            .selected
            .as_ref()
            .and_then(|file| entries.iter().position(|entry| &entry.file_name == file));
        let files: Vec<String> = entries.iter().map(|e| e.file_name.clone()).collect();
        let (select_files, menu_files) = (files.clone(), files);
        let table = DetailsTable::new(
            "startup",
            vec![
                Column::new(c.column_name),
                Column::new(c.column_status).width(px(180.0)),
                Column::new(c.column_starts_for).width(px(180.0)),
                Column::new(c.column_command),
            ],
            rows,
        )
        .fill()
        .sort(self.startup_sort)
        .selected(selected)
        .empty(if self.startup.loaded {
            c.no_startup_apps
        } else {
            c.loading
        })
        .on_sort(Self::act_with(cx, |this, sort: SortState, _, _| {
            this.startup_sort = Some(sort)
        }))
        .on_select(Self::act_with(cx, move |this, index: usize, _, _| {
            this.startup.selected = select_files.get(index).cloned();
        }))
        .on_context_menu(Self::act_point(
            cx,
            move |this, index, position, window, cx| {
                if let Some(file) = menu_files.get(index).cloned() {
                    this.startup.selected = Some(file.clone());
                    this.open_menu(MenuTarget::Startup(file), position, window, cx);
                }
            },
        ));
        div()
            .flex()
            .flex_col()
            .gap(spacing::MD)
            .size_full()
            .min_h_0()
            .child(label(c.startup_hint, BODY, palette(cx).text_secondary))
            .child(div().flex_1().min_h_0().flex().flex_col().child(table))
            .into_any_element()
    }

    // Services.

    fn services_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        if let Some(error) = &self.services.error {
            let detail = i18n::service_error_detail(error, self.locale);
            return div()
                .flex()
                .flex_col()
                .gap(spacing::MD)
                .child(
                    better_ui::fluent::info_bar::ErrorBar::new(
                        "services-error",
                        better_ui::fluent::info_bar::ErrorReport::new(c.no_service_manager, detail)
                            .advice(c.no_service_manager_advice),
                    )
                    .locale(self.locale.words()),
                )
                .into_any_element();
        }
        let services = self.services.services.clone();
        let rows: Vec<Vec<TableCell>> = services
            .iter()
            .map(|service| {
                let (startup, tone) = i18n::startup_word(service, self.locale);
                vec![
                    TableCell::new(service.title().to_string()),
                    TableCell::new(service.unit.clone()).tone(CellTone::Secondary),
                    {
                        let (word, tone) = i18n::activity_word(service.activity, self.locale);
                        TableCell::new(word).tone(tone)
                    },
                    TableCell::new(startup).tone(tone).tooltip(
                        service
                            .lock()
                            .map(|lock| i18n::lock_sentence(lock, self.locale))
                            .unwrap_or_default(),
                    ),
                ]
            })
            .collect();
        let selected = self
            .services
            .selected
            .as_ref()
            .and_then(|unit| services.iter().position(|s| &s.unit == unit));
        let units: Vec<String> = services.iter().map(|s| s.unit.clone()).collect();
        let (select_units, menu_units) = (units.clone(), units);
        let table = DetailsTable::new(
            "services",
            vec![
                Column::new(c.column_name),
                Column::new(c.column_service_name),
                Column::new(c.column_status).width(px(120.0)),
                Column::new(c.column_at_login).width(px(170.0)),
            ],
            rows,
        )
        .fill()
        .sort(self.services_sort)
        .selected(selected)
        .empty(if self.services.loaded {
            c.no_services
        } else {
            c.loading
        })
        .on_sort(Self::act_with(cx, |this, sort: SortState, _, _| {
            this.services_sort = Some(sort)
        }))
        .on_select(Self::act_with(cx, move |this, index: usize, _, _| {
            this.services.selected = select_units.get(index).cloned();
        }))
        .on_context_menu(Self::act_point(
            cx,
            move |this, index, position, window, cx| {
                if let Some(unit) = menu_units.get(index).cloned() {
                    this.services.selected = Some(unit.clone());
                    this.open_menu(MenuTarget::Service(unit), position, window, cx);
                }
            },
        ));
        div()
            .flex()
            .flex_col()
            .gap(spacing::MD)
            .size_full()
            .min_h_0()
            .child(label(c.services_hint, BODY, p.text_secondary))
            .child(div().flex_1().min_h_0().flex().flex_col().child(table))
            .into_any_element()
    }

    // Settings.

    fn settings_page(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let words = self.locale.words();
        let appearance = ComboBox::new(
            "appearance",
            vec![
                ComboItem::new(c.dark_theme),
                ComboItem::new(c.light_theme),
                ComboItem::new(c.system_theme),
            ],
            Appearance::ALL
                .iter()
                .position(|choice| *choice == self.appearance),
        )
        .min_width(px(200.0))
        .on_select(Self::act_with(cx, |this, index: usize, window, cx| {
            if let Some(choice) = Appearance::ALL.get(index) {
                this.set_appearance(*choice, window, cx);
            }
        }));
        let language = ComboBox::new(
            "language",
            vec![
                ComboItem::new(c.system_default),
                ComboItem::new("English"),
                ComboItem::new("繁體中文"),
            ],
            i18n::Locale::ALL.iter().position(|l| *l == self.locale),
        )
        .min_width(px(200.0))
        .on_select(Self::act_with(cx, |this, index: usize, window, cx| {
            if let Some(locale) = i18n::Locale::ALL.get(index) {
                this.set_locale(*locale, window, cx);
            }
        }));
        let command_lines = ToggleSwitch::new("command-lines", self.include_command_lines)
            .locale(words)
            .on_change(Self::act_with(cx, |this, on: bool, _, cx| {
                this.set_command_lines(on, cx)
            }));

        let mut grouping = SettingsExpander::new("grouping", c.grouping_title, self.grouping_open)
            .icon(IconName::GalleryVerticalEnd)
            .description(c.grouping_description)
            .on_toggle(Self::act_with(cx, |this, open: bool, _, _| {
                this.grouping_open = open
            }));
        for (position, kind) in self.precedence.order().iter().enumerate() {
            grouping = grouping.row(
                SettingsRow::new(format!(
                    "{}. {}",
                    position + 1,
                    i18n::evidence_kind_word(*kind, self.locale)
                ))
                .description(i18n::evidence_kind_explanation(*kind, self.locale)),
            );
        }

        let source = match &self.collection {
            crate::app::Collection::Connecting => c.collection_connecting,
            crate::app::Collection::Service => c.collection_service,
            crate::app::Collection::InProcess { .. } => c.collection_in_process,
            crate::app::Collection::Unavailable { .. } => c.collection_unavailable,
        };
        let mut sources = SettingsExpander::new("sources", c.data_sources, self.sources_open)
            .icon(IconName::CircleCheck)
            .description(source)
            .on_toggle(Self::act_with(cx, |this, open: bool, _, _| {
                this.sources_open = open
            }));
        for status in &self.overview.collectors {
            let (word, _) = match &status.health {
                CollectorHealth::Healthy => (c.health_healthy, ()),
                CollectorHealth::Degraded { .. } => (c.health_degraded, ()),
                CollectorHealth::Failed { .. } => (c.health_failed, ()),
                CollectorHealth::Unsupported(_) => (c.health_unsupported, ()),
            };
            sources = sources.row(
                SettingsRow::new(i18n::collector_name(&status.collector, self.locale))
                    .content(label(word, BODY, p.text_secondary)),
            );
        }
        let coverage = self.overview.coverage;
        sources = sources.row(SettingsRow::new(c.readings_this_second).description(
            i18n::coverage_sentence(
                coverage.value,
                coverage.stale + coverage.unknown,
                coverage.unsupported,
                coverage.permission_denied,
                self.locale,
            ),
        ));

        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .max_w(px(1000.0))
            .child(section_title(c.appearance_and_language, p))
            .child(
                SettingsCard::new("appearance-card", c.appearance)
                    .icon(IconName::Palette)
                    .description(c.appearance_description)
                    .content(appearance),
            )
            .child(
                SettingsCard::new("language-card", c.language)
                    .icon(IconName::Globe)
                    .description(c.language_description)
                    .content(language),
            )
            .child(section_title(c.privacy, p))
            .child(
                SettingsCard::new("command-lines-card", c.show_command_lines)
                    .icon(IconName::SquareTerminal)
                    .description(c.privacy_description)
                    .content(command_lines),
            )
            .child(section_title(c.how_it_works, p))
            .child(grouping)
            .child(sources)
            .child(
                SettingsCard::new("sampling-card", c.sampling)
                    .icon(IconName::Loader)
                    .description(c.sampling_description),
            )
            .child(
                SettingsCard::new("about-card", c.app_name)
                    .icon(IconName::Info)
                    .description(i18n::version_line(env!("CARGO_PKG_VERSION"), self.locale)),
            )
            .into_any_element()
    }
}

/// A grid of charts with 4px gaps, for the per-processor view.
fn grid_tight(columns: usize, children: Vec<AnyElement>) -> AnyElement {
    let columns = columns.max(1);
    let mut rows = div().flex().flex_col().gap(px(4.0)).w_full();
    let mut iter = children.into_iter().peekable();
    while iter.peek().is_some() {
        let mut row = div().flex().gap(px(4.0)).w_full();
        for _ in 0..columns {
            row = row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .when_some(iter.next(), |cell, child| cell.child(child)),
            );
        }
        rows = rows.child(row);
    }
    rows.into_any_element()
}

/// One part of the memory composition's legend: a swatch, the name, the
/// amount.
fn legend_item(
    name: &'static str,
    swatch: Rgba,
    value: Option<u64>,
    c: &'static Copy,
    p: &Palette,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(spacing::SM)
        .child(
            div()
                .size(px(12.0))
                .rounded(px(2.0))
                .border_1()
                .border_color(p.card_stroke)
                .bg(swatch),
        )
        .child(label(name, BODY, p.text_secondary))
        .child(label(
            value
                .map(|v| bytes(v as f64))
                .unwrap_or_else(|| c.not_available.to_string()),
            BODY_STRONG,
            p.text_primary,
        ))
        .into_any_element()
}

class_name AnalysisTab
extends VBoxContainer
## The Analysis tab (R61): the active rider's power across their rides, for a period and in
## watts or watts per kilogram. The power curve (R62) shows the best power over every duration
## among the period's rides, against all rides or the period before, with the durations riders
## compare below; beside it the power profile (R63) sets the period against the rider's best.
## Below them: fitness, fatigue and form over the period's days and its weeks' totals (R64).

## The periods to choose from, in days (0: all rides).
# i18n-begin
const PERIODS: Array[Array] = [
	["Last 6 weeks", 42], ["Last 90 days", 90], ["Last 12 months", 365], ["All time", 0]
]
## What the period is compared with: the choice, and its name in the legend.
const COMPARISONS: Array[Array] = [
	["Against all time", "All time"], ["Against the period before", "The period before"]
]
## What the weekly bars show: totals, or the time in each zone stacked.
const WEEKLY: Array[String] = [
	"Time",
	"Distance",
	"Climbing",
	"Training stress",
	"Rides",
	"Time in power zones",
	"Time in heart-rate zones",
]
# i18n-end
const KM_PER_MILE: float = 1.609344
const METERS_PER_FOOT: float = 0.3048
## The durations shown as figures under the curve, in seconds (the records of R62).
const KEY_DURATIONS: Array[int] = [5, 60, 300, 1200, 3600]

var _torqa: TorqaApp
var _period: OptionButton = OptionButton.new()
var _compare: OptionButton = OptionButton.new()
var _per_kg: CheckBox = CheckBox.new()
var _scroll: ScrollContainer = ScrollContainer.new()
## The sections under the heading, hidden without rides with power.
var _content: VBoxContainer = VBoxContainer.new()
var _empty: Label = Label.new()
var _chart: PowerCurveChart = PowerCurveChart.new()
var _legend: HBoxContainer = HBoxContainer.new()
var _figures: GridContainer = GridContainer.new()
var _radar: RadarChart = RadarChart.new()
var _fitness: FitnessChart = FitnessChart.new()
## Fitness, fatigue and form today.
var _today: GridContainer = GridContainer.new()
var _weekly_metric: OptionButton = OptionButton.new()
var _weekly: WeeklyBars = WeeklyBars.new()
var _zone_legend: HFlowContainer = HFlowContainer.new()
## The period's weeks as `TorqaApp.weekly()` gives them, kept to switch what the bars show.
var _weeks: Array = []
var _imperial: bool = false


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa


## Reads the active rider's rides again, e.g. when the tab is shown or after a ride.
func refresh() -> void:
	if _torqa == null:
		return
	var period: Array = PERIODS[_period.selected]
	var days: int = period[1]
	var previous: bool = _compare.selected == 1
	var per_kg: bool = _per_kg.button_pressed
	var curves: Dictionary = _torqa.power_curves(days, previous, per_kg)
	var current: Array = curves.get("period", [])
	var compared: Array = curves.get("compare", [])
	# All rides against all rides would draw the same curve twice.
	if days == 0 and not previous:
		compared = []
	_empty.visible = current.is_empty() and compared.is_empty()
	_content.visible = not _empty.visible
	_chart.unit = "W/kg" if per_kg else "W"
	_chart.clear()
	_chart.add_efforts(current, UiTheme.POWER_COLOR, per_kg)
	_chart.add_efforts(compared, UiTheme.MUTED, per_kg)
	var period_name: String = period[0]
	var comparison: Array = COMPARISONS[_compare.selected]
	var compare_name: String = "" if compared.is_empty() else comparison[1]
	_fill_legend(period_name, compare_name)
	_fill_figures(current, compared, per_kg)
	_fill_radar(_torqa.power_profile(days, per_kg), per_kg)
	var bias_minutes: int = Time.get_time_zone_from_system().get("bias", 0)
	var loads: Dictionary = _torqa.fitness(days, bias_minutes)
	_fitness.set_days(loads)
	_fill_today(loads)
	_imperial = _torqa.profile().get("units", "metric") == "imperial"
	_weeks = _torqa.weekly(days, bias_minutes)
	_fill_weekly()


func _init() -> void:
	_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	# Room between the page and its scroll bar, which would cover the period's choice.
	_scroll.add_theme_constant_override("scrollbar_h_separation", 12)
	add_child(_scroll)
	# The period applies to every section; it scrolls away with them, a choice at the top of the
	# page rather than a bar over all of it. Each section has its own title.
	var page: VBoxContainer = VBoxContainer.new()
	page.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	page.add_theme_constant_override("separation", 16)
	_scroll.add_child(page)
	var header: HBoxContainer = HBoxContainer.new()
	header.alignment = BoxContainer.ALIGNMENT_END
	for period: Array in PERIODS:
		var period_name: String = period[0]
		_period.add_item(tr(period_name))
	_period.select(0)
	_period.item_selected.connect(func(_index: int) -> void: refresh())
	header.add_child(_period)
	page.add_child(header)

	_empty.text = tr(
		"No rides with power yet: your best efforts show here once you ride with power."
	)
	_empty.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_empty.add_theme_color_override("font_color", UiTheme.MUTED)
	_empty.hide()
	page.add_child(_empty)

	_content.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_content.add_theme_constant_override("separation", 16)
	page.add_child(_content)
	_content.add_child(_power_card())
	_content.add_child(_fitness_card())
	_content.add_child(_weekly_card())


## Power in one card (R62, R63): a row with what the curve is compared with and whether power is
## per kilogram, then the curve and the profile side by side.
func _power_card() -> PanelContainer:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 12)
	var settings: HBoxContainer = HBoxContainer.new()
	settings.alignment = BoxContainer.ALIGNMENT_END
	settings.add_theme_constant_override("separation", 16)
	# Switches the curve, the profile and the progress.
	_per_kg.text = tr("Per kilogram")
	_per_kg.toggled.connect(func(_on: bool) -> void: refresh())
	settings.add_child(_per_kg)
	for comparison: Array in COMPARISONS:
		var choice: String = comparison[0]
		_compare.add_item(tr(choice))
	_compare.item_selected.connect(func(_index: int) -> void: refresh())
	settings.add_child(_compare)
	rows.add_child(settings)
	var charts: HBoxContainer = HBoxContainer.new()
	charts.add_theme_constant_override("separation", 32)
	charts.add_child(_curve_column())
	charts.add_child(_profile_column())
	rows.add_child(charts)
	return _card(rows)


## The power curve (R62) with its legend and the key durations.
func _curve_column() -> VBoxContainer:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	rows.add_theme_constant_override("separation", 12)
	rows.add_child(UiTheme.caption(tr("Power curve")))
	var about: Label = Label.new()
	about.text = tr("The best power you held for each length of effort, from a sprint to hours.")
	about.add_theme_color_override("font_color", UiTheme.MUTED)
	rows.add_child(about)
	_legend.add_theme_constant_override("separation", 18)
	rows.add_child(_legend)
	_chart.custom_minimum_size = Vector2(0, 320)
	rows.add_child(_chart)
	_figures.columns = KEY_DURATIONS.size()
	_figures.add_theme_constant_override("h_separation", 40)
	rows.add_child(_figures)
	return rows


## The power profile (R63): the period's best against the rider's best of all.
func _profile_column() -> VBoxContainer:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 12)
	rows.add_child(UiTheme.caption(tr("Power profile")))
	var about: Label = Label.new()
	# On one line: it sets the column's width, the curve beside it takes the rest.
	about.text = tr("Your best of the period as a share of your best of all time.")
	about.add_theme_color_override("font_color", UiTheme.MUTED)
	rows.add_child(about)
	_radar.custom_minimum_size = Vector2(380, 340)
	_radar.size_flags_vertical = Control.SIZE_EXPAND_FILL
	rows.add_child(_radar)
	return rows


## Fitness, fatigue and form (R64) over the period's days, and where they stand today.
func _fitness_card() -> PanelContainer:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 12)
	rows.add_child(UiTheme.caption(tr("Fitness, fatigue and form")))
	var about: Label = Label.new()
	about.text = (
		tr("Your training load from the rides' TSS: fitness over six weeks, fatigue over one.")
		+ " "
		+ tr("Form is fitness minus fatigue: fresh above zero, tired below it.")
	)
	about.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	about.add_theme_color_override("font_color", UiTheme.MUTED)
	rows.add_child(about)
	var legend: HBoxContainer = HBoxContainer.new()
	legend.add_theme_constant_override("separation", 18)
	# i18n-begin
	for entry: Array in [
		["Fitness", FitnessChart.FITNESS_COLOR],
		["Fatigue", FitnessChart.FATIGUE_COLOR],
		["Form", FitnessChart.FORM_COLOR],
		["Daily TSS", UiTheme.MUTED],
	]:
		# i18n-end
		var key_name: String = entry[0]
		var key: Label = UiTheme.caption(key_name)
		var color: Color = entry[1]
		key.add_theme_color_override("font_color", color)
		legend.add_child(key)
	rows.add_child(legend)
	_fitness.custom_minimum_size = Vector2(0, 320)
	rows.add_child(_fitness)
	_today.columns = 3
	_today.add_theme_constant_override("h_separation", 40)
	rows.add_child(_today)
	return _card(rows)


## The period's weeks (R64): time, distance, climbing, TSS, rides or time in zones.
func _weekly_card() -> PanelContainer:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 12)
	var title: HBoxContainer = HBoxContainer.new()
	var caption: Label = UiTheme.caption(tr("Weekly"))
	caption.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	title.add_child(caption)
	for metric: String in WEEKLY:
		_weekly_metric.add_item(tr(metric))
	_weekly_metric.item_selected.connect(func(_index: int) -> void: _fill_weekly())
	title.add_child(_weekly_metric)
	rows.add_child(title)
	_weekly.custom_minimum_size = Vector2(0, 240)
	rows.add_child(_weekly)
	_zone_legend.add_theme_constant_override("h_separation", 18)
	rows.add_child(_zone_legend)
	return _card(rows)


static func _card(content: Control) -> PanelContainer:
	var card: PanelContainer = PanelContainer.new()
	card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	card.add_child(content)
	return card


func _fill_legend(period_name: String, compare_name: String) -> void:
	for child: Node in _legend.get_children():
		child.queue_free()
	for entry: Array in [[period_name, UiTheme.POWER_COLOR], [compare_name, UiTheme.MUTED]]:
		var entry_name: String = entry[0]
		if entry_name.is_empty():
			continue
		var key: Label = UiTheme.caption(entry_name)
		var color: Color = entry[1]
		key.add_theme_color_override("font_color", color)
		_legend.add_child(key)


## The period's best over each key duration, the comparison's under it.
func _fill_figures(current: Array, compared: Array, per_kg: bool) -> void:
	for child: Node in _figures.get_children():
		child.queue_free()
	for seconds: int in KEY_DURATIONS:
		var cell: VBoxContainer = VBoxContainer.new()
		cell.add_theme_constant_override("separation", 0)
		cell.add_child(UiTheme.caption(PowerCurveChart.duration_label(seconds)))
		var value: Label = UiTheme.value(22)
		value.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		value.text = _value_text(_effort(current, seconds), per_kg)
		cell.add_child(value)
		var other: Dictionary = _effort(compared, seconds)
		var below: Label = Label.new()
		below.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		below.text = "" if other.is_empty() else _value_text(other, per_kg)
		below.add_theme_color_override("font_color", UiTheme.MUTED)
		cell.add_child(below)
		_figures.add_child(cell)


## Each spoke's share of the rider's best and the period's value, as `TorqaApp.power_profile()`
## gives them.
func _fill_radar(axes: Array, per_kg: bool) -> void:
	var labels: PackedStringArray = PackedStringArray()
	var shares: PackedFloat64Array = PackedFloat64Array()
	var values: PackedStringArray = PackedStringArray()
	for axis: Dictionary in axes:
		var duration_s: float = axis["duration_s"]
		labels.append(PowerCurveChart.duration_label(duration_s))
		var share: float = axis["share"] if axis["share"] != null else -1.0
		shares.append(share)
		var value: Variant = axis["period"]
		values.append(_number_text(value, per_kg))
	_radar.set_spokes(labels, shares, values)


## Today's fitness, fatigue and form, the last of `loads` (`TorqaApp.fitness()`).
func _fill_today(loads: Dictionary) -> void:
	for child: Node in _today.get_children():
		child.queue_free()
	var fitness: PackedFloat64Array = loads.get("fitness", PackedFloat64Array())
	var fatigue: PackedFloat64Array = loads.get("fatigue", PackedFloat64Array())
	var form: PackedFloat64Array = loads.get("form", PackedFloat64Array())
	if form.is_empty():
		return
	var last: int = form.size() - 1
	# i18n-begin
	var figures: Array[Array] = [
		["Fitness", "%d" % roundi(fitness[last]), ""],
		["Fatigue", "%d" % roundi(fatigue[last]), ""],
		["Form", "%+d" % roundi(form[last]), FitnessChart.band(form[last])],
	]
	# i18n-end
	for figure: Array in figures:
		var cell: VBoxContainer = VBoxContainer.new()
		cell.add_theme_constant_override("separation", 0)
		var caption: String = figure[0]
		cell.add_child(UiTheme.caption(caption))
		var value: Label = UiTheme.value(22)
		value.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		value.text = figure[1]
		cell.add_child(value)
		var note: Label = Label.new()
		var band_name: String = figure[2]
		note.text = tr(band_name) if not band_name.is_empty() else ""
		note.add_theme_color_override("font_color", UiTheme.MUTED)
		cell.add_child(note)
		_today.add_child(cell)


## The weeks' bars for the metric chosen, from `_weeks`.
func _fill_weekly() -> void:
	var starts: PackedInt64Array = PackedInt64Array()
	var stacks: Array[PackedFloat64Array] = []
	var metric: int = _weekly_metric.selected
	for week: Dictionary in _weeks:
		var start: int = week["start_unix_s"]
		starts.append(start)
		stacks.append(_week_values(week, metric))
	var colors: Array[Color] = [UiTheme.POWER_COLOR]
	var names: PackedStringArray = PackedStringArray()
	var zones: Array[Array] = []
	if metric == 5:
		zones = UiTheme.POWER_ZONES
	elif metric == 6:
		zones = UiTheme.HEART_RATE_ZONES
	if not zones.is_empty():
		colors.clear()
		for i: int in range(zones.size()):
			var color: Color = zones[i][1]
			colors.append(color)
			names.append("Z%d" % (i + 1))
	_weekly.set_weeks(starts, stacks, colors, names, _week_format(metric))
	_fill_zone_legend(zones)


## One week's values for `metric` (an index of `WEEKLY`): one, or one per zone, in the units
## the bars show (hours, km or mi, m or ft).
func _week_values(week: Dictionary, metric: int) -> PackedFloat64Array:
	var time_s: float = week["time_s"]
	var distance_m: float = week["distance_m"]
	var climbing_m: float = week["climbing_m"]
	var stress: float = week["stress"]
	var rides: int = week["rides"]
	match metric:
		0:
			return PackedFloat64Array([time_s / 3600.0])
		1:
			var km: float = distance_m / 1000.0
			return PackedFloat64Array([km / KM_PER_MILE if _imperial else km])
		2:
			return PackedFloat64Array([climbing_m / METERS_PER_FOOT if _imperial else climbing_m])
		3:
			return PackedFloat64Array([stress])
		4:
			return PackedFloat64Array([float(rides)])
	var seconds: PackedFloat64Array = week["power_zones" if metric == 5 else "heart_rate_zones"]
	var hours: PackedFloat64Array = PackedFloat64Array()
	for value: float in seconds:
		hours.append(value / 3600.0)
	return hours


## How the bars write a value of `metric`.
func _week_format(metric: int) -> Callable:
	match metric:
		1:
			var distance_unit: String = "mi" if _imperial else "km"
			return func(value: float) -> String: return "%d %s" % [roundi(value), distance_unit]
		2:
			var climbing_unit: String = "ft" if _imperial else "m"
			return func(value: float) -> String: return "%d %s" % [roundi(value), climbing_unit]
		3, 4:
			return func(value: float) -> String: return "%d" % roundi(value)
	return func(value: float) -> String: return _hours(value)


## The zones' names in their colours under stacked bars; nothing for totals.
func _fill_zone_legend(zones: Array[Array]) -> void:
	for child: Node in _zone_legend.get_children():
		child.queue_free()
	for i: int in range(zones.size()):
		var zone_name: String = zones[i][0]
		var key: Label = Label.new()
		key.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		key.text = "Z%d %s" % [i + 1, tr(zone_name)]
		var color: Color = zones[i][1]
		key.add_theme_color_override("font_color", color)
		key.add_theme_font_size_override("font_size", 12)
		_zone_legend.add_child(key)


## Hours as "4:12 h".
static func _hours(hours: float) -> String:
	var minutes: int = roundi(hours * 60.0)
	return "%d:%02d h" % [minutes / 60, minutes % 60]


static func _effort(efforts: Array, seconds: int) -> Dictionary:
	for effort: Dictionary in efforts:
		var duration_s: float = effort["duration_s"]
		if roundi(duration_s) == seconds:
			return effort
	return {}


static func _value_text(effort: Dictionary, per_kg: bool) -> String:
	if effort.is_empty():
		return "--"
	return _number_text(effort["watts_per_kg"] if per_kg else effort["power"], per_kg)


## Power as "312 W" or "4.5 W/kg"; "--" for `null`.
static func _number_text(value: Variant, per_kg: bool) -> String:
	if value == null:
		return "--"
	var number: float = value
	return "%.1f W/kg" % number if per_kg else "%d W" % roundi(number)

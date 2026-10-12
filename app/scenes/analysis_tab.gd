class_name AnalysisTab
extends VBoxContainer
## The Analysis tab (R61): the active rider's power across their rides, for a period and in
## watts or watts per kilogram. The power curve (R62) shows the best power over every duration
## among the period's rides, against all rides or the period before, with the durations riders
## compare below; beside it the power profile (R63) sets the period against the rider's best.

## The periods to choose from, in days (0: all rides).
# i18n-begin
const PERIODS: Array[Array] = [
	["Last 6 weeks", 42], ["Last 90 days", 90], ["Last 12 months", 365], ["All time", 0]
]
## What the period is compared with: the choice, and its name in the legend.
const COMPARISONS: Array[Array] = [
	["Against all time", "All time"], ["Against the period before", "The period before"]
]
# i18n-end
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


func _init() -> void:
	_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	add_child(_scroll)
	# The heading and its choices scroll away with the sections: the page's title, not a bar
	# over sections that are not about power.
	var page: VBoxContainer = VBoxContainer.new()
	page.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	page.add_theme_constant_override("separation", 16)
	_scroll.add_child(page)
	var header: HBoxContainer = HBoxContainer.new()
	header.add_theme_constant_override("separation", 12)
	var heading: Label = Label.new()
	heading.text = tr("Your power")
	heading.add_theme_font_size_override("font_size", 22)
	heading.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header.add_child(heading)
	for period: Array in PERIODS:
		var period_name: String = period[0]
		_period.add_item(tr(period_name))
	_period.select(0)
	_period.item_selected.connect(func(_index: int) -> void: refresh())
	header.add_child(_period)
	_per_kg.text = tr("Per kilogram")
	_per_kg.toggled.connect(func(_on: bool) -> void: refresh())
	header.add_child(_per_kg)
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
	var power: HBoxContainer = HBoxContainer.new()
	power.add_theme_constant_override("separation", 16)
	power.add_child(_curve_card())
	power.add_child(_profile_card())
	_content.add_child(power)


## The power curve (R62) with what it is compared with, its legend and the key durations.
func _curve_card() -> PanelContainer:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 12)
	var title: HBoxContainer = HBoxContainer.new()
	var caption: Label = UiTheme.caption(tr("Power curve"))
	caption.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	title.add_child(caption)
	for comparison: Array in COMPARISONS:
		var choice: String = comparison[0]
		_compare.add_item(tr(choice))
	_compare.item_selected.connect(func(_index: int) -> void: refresh())
	title.add_child(_compare)
	rows.add_child(title)
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
	return _card(rows, true)


## The power profile (R63): the period's best against the rider's best of all.
func _profile_card() -> PanelContainer:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 12)
	rows.add_child(UiTheme.caption(tr("Power profile")))
	var about: Label = Label.new()
	about.text = tr("Your best of the period as a share of your best of all time.")
	about.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	about.add_theme_color_override("font_color", UiTheme.MUTED)
	rows.add_child(about)
	_radar.custom_minimum_size = Vector2(380, 340)
	_radar.size_flags_vertical = Control.SIZE_EXPAND_FILL
	rows.add_child(_radar)
	return _card(rows, false)


static func _card(content: Control, expand: bool) -> PanelContainer:
	var card: PanelContainer = PanelContainer.new()
	if expand:
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

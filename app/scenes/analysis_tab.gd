class_name AnalysisTab
extends VBoxContainer
## The Analysis tab (R61): the active rider's power across their rides. The power curve (R62)
## shows the best power over every duration among the rides of a period, against all rides or
## the period before, in watts or watts per kilogram, with the durations riders compare below.

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
var _content: VBoxContainer = VBoxContainer.new()
var _empty: Label = Label.new()
var _chart: PowerCurveChart = PowerCurveChart.new()
var _legend: HBoxContainer = HBoxContainer.new()
var _figures: GridContainer = GridContainer.new()


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


func _init() -> void:
	add_theme_constant_override("separation", 16)
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
	for comparison: Array in COMPARISONS:
		var choice: String = comparison[0]
		_compare.add_item(tr(choice))
	_compare.item_selected.connect(func(_index: int) -> void: refresh())
	header.add_child(_compare)
	_per_kg.text = tr("Per kilogram")
	_per_kg.toggled.connect(func(_on: bool) -> void: refresh())
	header.add_child(_per_kg)
	add_child(header)

	_empty.text = tr(
		"No rides with power yet: your best efforts show here once you ride with power."
	)
	_empty.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_empty.add_theme_color_override("font_color", UiTheme.MUTED)
	_empty.hide()
	add_child(_empty)

	var card: PanelContainer = PanelContainer.new()
	card.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_content.add_theme_constant_override("separation", 12)
	card.add_child(_content)
	add_child(card)
	_content.add_child(UiTheme.caption(tr("Power curve")))
	var about: Label = Label.new()
	about.text = tr("The best power you held for each length of effort, from a sprint to hours.")
	about.add_theme_color_override("font_color", UiTheme.MUTED)
	_content.add_child(about)
	_legend.add_theme_constant_override("separation", 18)
	_content.add_child(_legend)
	_chart.custom_minimum_size = Vector2(0, 320)
	_chart.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_content.add_child(_chart)
	_figures.columns = KEY_DURATIONS.size()
	_figures.add_theme_constant_override("h_separation", 40)
	_content.add_child(_figures)


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


static func _effort(efforts: Array, seconds: int) -> Dictionary:
	for effort: Dictionary in efforts:
		var duration_s: float = effort["duration_s"]
		if roundi(duration_s) == seconds:
			return effort
	return {}


static func _value_text(effort: Dictionary, per_kg: bool) -> String:
	if effort.is_empty():
		return "--"
	if per_kg:
		var ratio: float = effort["watts_per_kg"]
		return "%.1f W/kg" % ratio
	var watts: float = effort["power"]
	return "%d W" % roundi(watts)

class_name HudPanel
extends VBoxContainer
## The ride HUD's figures: the first metric of a layout large, the others in a grid (R23). Used
## live during rides and, with sample values, as the preview in the HUD editor (R51). When
## `editable`, figures are dragged in, around and out of it directly (R54).

## Editing: a figure was dropped on the HUD and should go to `index` of the layout.
signal drop_requested(id: String, index: int)

const KM_PER_MILE: float = 1.609344
const METERS_PER_FOOT: float = 0.3048
const POWER_METRICS: Array[String] = ["power", "power_3s", "power_10s"]
## A large heart rate gets its % of the maximum and zone underneath, as power gets its own.
const HEART_RATE_METRIC: String = "heart_rate"
## Drag data carries the metric id under this key.
const DRAG_KEY: String = "hud_metric"

## Metric descriptions by id, from `OrqwaApp.hud_metrics()`.
var catalogue: Dictionary[String, Dictionary] = {}
## Speed, distance and elevation in imperial units.
var imperial: bool = false
## Figures can be dragged into, around and out of the HUD.
var editable: bool = false:
	set(value):
		editable = value
		mouse_filter = Control.MOUSE_FILTER_STOP if value else Control.MOUSE_FILTER_PASS
## The cell showing where a dragged figure would land.
var _indicated: _Cell = null
var _layout: PackedStringArray = PackedStringArray()
var _values: Dictionary[String, Label] = {}
## The grid's figures by id; the large one is left out of the zone bars, its line underneath
## already coloured by zone.
var _cells: Dictionary[String, _Cell] = {}
## The line under a large power or heart-rate figure.
var _detail: Label = Label.new()


func _init() -> void:
	add_theme_constant_override("separation", 12)
	for metric: Dictionary in OrqwaApp.hud_metrics():
		var id: String = metric["id"]
		catalogue[id] = metric
	# Kept in the tree between layouts, so it is freed with the panel.
	_detail.hide()
	_detail.add_theme_font_size_override("font_size", 13)
	add_child(_detail)


## Rebuilds the figures for `layout`; values show "--" until `show_values` or `show_samples`.
func show_layout(layout: PackedStringArray) -> void:
	_layout = layout
	_values.clear()
	_cells.clear()
	# The W/kg line moves to the new large figure rather than being freed with the old one.
	_detail.get_parent().remove_child(_detail)
	for child: Node in get_children():
		remove_child(child)
		child.queue_free()
	_detail.text = ""
	_detail.custom_minimum_size.x = 0.0
	if layout.is_empty():
		_detail.hide()
		add_child(_detail)
		return
	var main_id: String = layout[0]
	var main: _Cell = _Cell.new(self, main_id, 0, false)
	main.add_child(UiTheme.caption(_caption(main_id)))
	main.add_child(_value_row(main_id, 48))
	# Power figures get the rider's W/kg, % of FTP and zone underneath; heart rate its % of the
	# maximum and zone.
	_detail.visible = main_id in POWER_METRICS or main_id == HEART_RATE_METRIC
	main.add_child(_detail)
	add_child(main)
	if layout.size() == 1:
		return

	var divider: ColorRect = ColorRect.new()
	divider.color = Color(1, 1, 1, 0.08)
	divider.custom_minimum_size = Vector2(0, 1)
	add_child(divider)

	var grid: GridContainer = GridContainer.new()
	grid.columns = 2
	grid.add_theme_constant_override("h_separation", 24)
	grid.add_theme_constant_override("v_separation", 10)
	for i: int in range(1, layout.size()):
		var id: String = layout[i]
		var cell: _Cell = _Cell.new(self, id, i, true)
		cell.custom_minimum_size = Vector2(98, 0)
		_cells[id] = cell
		cell.add_child(UiTheme.caption(_caption(id)))
		cell.add_child(_value_row(id, 24))
		grid.add_child(cell)
	add_child(grid)


## The metric id carried by drag `data`, or "" if it is not a HUD figure.
static func dragged(data: Variant) -> String:
	if typeof(data) != TYPE_DICTIONARY:
		return ""
	var fields: Dictionary = data
	var id: String = fields.get(DRAG_KEY, "")
	return id


## Dropping on free space in the HUD appends the figure.
func _can_drop_data(_at: Vector2, data: Variant) -> bool:
	if not editable or dragged(data).is_empty():
		return false
	_indicate(null, false)
	return true


func _drop_data(_at: Vector2, data: Variant) -> void:
	drop_requested.emit(dragged(data), _layout.size())


func _notification(what: int) -> void:
	if what == NOTIFICATION_DRAG_END:
		_indicate(null, false)


## Shows the drop position at `cell` (before it, or `after` it); none for null.
func _indicate(cell: _Cell, after: bool) -> void:
	if _indicated != null and _indicated != cell:
		_indicated.mark(0)
	_indicated = cell
	if cell != null:
		cell.mark(2 if after else 1)


## Live values (`ride_state()["metrics"]`), with W/kg and power zone for the power line; its
## % of FTP, and the heart rate's % of the maximum and zone, are among the values.
func show_values(values: Dictionary, watts_per_kg: Variant, power_zone: Variant) -> void:
	for id: String in _values:
		_values[id].text = _format(catalogue[id], values.get(id))
	if _layout.is_empty() or _layout[0] != HEART_RATE_METRIC:
		_show_power_detail(watts_per_kg, values.get("power_percent_ftp"), power_zone)
	else:
		_show_heart_rate_detail(values.get("heart_rate_percent_max"), values.get("heart_rate_zone"))


## The zone each figure's value lies in (`ride_state()["metric_zones"]`, #432), as a bar in
## the zone's colour down the left edge of the figures under the large one; none where a
## figure has no zone.
func show_zones(zones: Dictionary) -> void:
	for id: String in _cells:
		var scale: String = catalogue[id]["zones"]
		_cells[id].show_zone(_zone_color(scale, zones.get(id)))


## Typical values, so a layout can be judged before riding with it.
func show_samples() -> void:
	var samples: Dictionary = {}
	var zones: Dictionary = {}
	for id: String in catalogue:
		samples[id] = catalogue[id]["sample"]
		zones[id] = catalogue[id]["sample_zone"]
	show_values(samples, samples["watts_per_kg"], samples["power_zone"])
	show_zones(zones)


static func _zone_color(scale: String, zone: Variant) -> Color:
	if zone == null:
		return Color.TRANSPARENT
	var palette: Array[Array] = (
		UiTheme.HEART_RATE_ZONES if scale == "heart_rate" else UiTheme.POWER_ZONES
	)
	var number: int = zone
	return palette[clampi(number - 1, 0, palette.size() - 1)][1]


func _caption(id: String) -> String:
	var caption: String = catalogue[id]["caption"]
	return caption


func _value_row(id: String, size: int) -> HBoxContainer:
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 4)
	var value: Label = UiTheme.value(size)
	row.add_child(value)
	var unit: String = _unit(catalogue[id])
	if not unit.is_empty():
		var unit_label: Label = Label.new()
		unit_label.text = unit
		unit_label.size_flags_vertical = Control.SIZE_SHRINK_END
		unit_label.add_theme_font_size_override("font_size", maxi(11, size / 3))
		unit_label.add_theme_color_override("font_color", UiTheme.MUTED)
		row.add_child(unit_label)
	_values[id] = value
	return row


## The unit shown next to a metric, in the rider's unit system.
func _unit(metric: Dictionary) -> String:
	match metric["kind"]:
		"speed":
			return "mph" if imperial else "km/h"
		"distance":
			return "mi" if imperial else "km"
		"elevation":
			return "ft" if imperial else "m"
		"grade":
			return "%"
	return metric["unit"]


## A metric's value (km/h, km, m or s, as `ride_state()["metrics"]` has them) as display text.
func _format(metric: Dictionary, value: Variant) -> String:
	if value == null:
		return "--"
	var number: float = value
	var decimals: int = metric["decimals"]
	match metric["kind"]:
		"speed", "distance":
			if imperial:
				number /= KM_PER_MILE
		"elevation":
			if imperial:
				number /= METERS_PER_FOOT
		"duration":
			return UiTheme.duration(number)
		"grade":
			return "%+.1f" % number
		"zone":
			return "Z%d" % roundi(number)
	return "%.*f" % [decimals, number]


## "3.6 W/kg · 98 % FTP · Z4 Threshold", coloured by zone; empty without power, and without
## the percentage for a rider without an FTP.
func _show_power_detail(watts_per_kg: Variant, percent_ftp: Variant, zone: Variant) -> void:
	if watts_per_kg == null or zone == null:
		_detail.text = ""
		return
	var ratio: float = watts_per_kg
	var zone_number: int = zone
	var index: int = clampi(zone_number - 1, 0, UiTheme.POWER_ZONES.size() - 1)
	var zone_name: String = UiTheme.POWER_ZONES[index][0]
	var color: Color = UiTheme.POWER_ZONES[index][1]
	var parts: PackedStringArray = PackedStringArray(["%.1f W/kg" % ratio])
	if percent_ftp != null:
		var percent: float = percent_ftp
		parts.append("%d %% FTP" % roundi(percent))
	parts.append("Z%d %s" % [index + 1, tr(zone_name)])
	_show_detail(" · ".join(parts), color)


## "80 % max · Z3 Moderate", coloured by zone (#432); empty without a heart rate, or for a
## rider without the maximum the zones are set on.
func _show_heart_rate_detail(percent_max: Variant, zone: Variant) -> void:
	if percent_max == null or zone == null:
		_detail.text = ""
		return
	var percent: float = percent_max
	var zone_value: float = zone
	var zone_number: int = roundi(zone_value)
	var index: int = clampi(zone_number - 1, 0, UiTheme.HEART_RATE_ZONES.size() - 1)
	var zone_name: String = UiTheme.HEART_RATE_ZONES[index][0]
	var color: Color = UiTheme.HEART_RATE_ZONES[index][1]
	var parts: PackedStringArray = PackedStringArray(
		[tr("%d %% max") % roundi(percent), "Z%d %s" % [index + 1, tr(zone_name)]]
	)
	_show_detail(" · ".join(parts), color)


func _show_detail(text: String, color: Color) -> void:
	_detail.text = text
	_detail.add_theme_color_override("font_color", color)
	# Zone 7's line is wider than the HUD's grid. Keeping the widest width so far stops the HUD
	# from jumping in width each time a sprint crosses between zones 6 and 7.
	var width: float = _detail.get_minimum_size().x
	_detail.custom_minimum_size.x = maxf(_detail.custom_minimum_size.x, width)


## One figure of the HUD, under the large one with a bar in the colour of its value's zone down
## its left edge. In an editable HUD it can be dragged elsewhere, and dropping a figure on it
## places that one before or after it, shown by an accent line.
class _Cell:
	extends VBoxContainer

	const MARK_WIDTH: float = 3.0

	var _panel: HudPanel
	var _id: String
	var _index: int
	## Grid cells split left/right, the large figure top/bottom.
	var _sideways: bool
	## 0 none, 1 before, 2 after.
	var _mark: int = 0
	var _zone: Color = Color.TRANSPARENT

	func _init(panel: HudPanel, id: String, index: int, sideways: bool) -> void:
		_panel = panel
		_id = id
		_index = index
		_sideways = sideways
		add_theme_constant_override("separation", 0)
		if panel.editable:
			mouse_filter = Control.MOUSE_FILTER_STOP
			mouse_default_cursor_shape = Control.CURSOR_DRAG

	func show_zone(color: Color) -> void:
		if color != _zone:
			_zone = color
			queue_redraw()

	func mark(where: int) -> void:
		if where != _mark:
			_mark = where
			queue_redraw()

	func _after(at: Vector2) -> bool:
		return at.x > size.x / 2.0 if _sideways else at.y > size.y / 2.0

	func _get_drag_data(_at: Vector2) -> Variant:
		if not _panel.editable:
			return null
		var preview: Label = UiTheme.caption(_id)
		preview.text = (get_child(0) as Label).text
		set_drag_preview(preview)
		return {HudPanel.DRAG_KEY: _id}

	func _can_drop_data(at: Vector2, data: Variant) -> bool:
		if not _panel.editable or HudPanel.dragged(data).is_empty():
			return false
		_panel._indicate(self, _after(at))
		return true

	func _drop_data(at: Vector2, data: Variant) -> void:
		_panel._indicate(null, false)
		_panel.drop_requested.emit(HudPanel.dragged(data), _index + (1 if _after(at) else 0))

	func _draw() -> void:
		# Where the line before a grid figure goes while dragging, which then covers it.
		if _zone.a > 0.0:
			draw_rect(Rect2(-MARK_WIDTH - 4.0, 0.0, MARK_WIDTH, size.y), _zone)
		if _mark == 0:
			return
		var color: Color = UiTheme.ACCENT
		if _sideways:
			var x: float = -MARK_WIDTH - 4.0 if _mark == 1 else size.x + 4.0
			draw_rect(Rect2(x, 0.0, MARK_WIDTH, size.y), color)
		else:
			var y: float = -MARK_WIDTH - 2.0 if _mark == 1 else size.y + 2.0
			draw_rect(Rect2(0.0, y, size.x, MARK_WIDTH), color)

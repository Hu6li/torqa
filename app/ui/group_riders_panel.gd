class_name GroupRidersPanel
extends PanelContainer
## Current group members beside the map: who is speaking, their power and W/kg (#422) and
## locally interpolated distance gaps.

var _rows: GridContainer = GridContainer.new()
var _labels: Dictionary[int, Label] = {}
var _efforts: Dictionary[int, Label] = {}
var _per_kg: Dictionary[int, Label] = {}
var _speaking: Dictionary[int, TextureRect] = {}
var _key: String = ""
var _title: Label = UiTheme.caption("")


func _ready() -> void:
	var content: VBoxContainer = VBoxContainer.new()
	content.add_theme_constant_override("separation", 8)
	_title.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	content.add_child(_title)
	_rows.columns = 4
	_rows.add_theme_constant_override("h_separation", 8)
	_rows.add_theme_constant_override("v_separation", 8)
	content.add_child(_rows)
	var style: StyleBoxFlat = UiTheme.panel()
	style.content_margin_top = 12
	style.content_margin_bottom = 12
	add_theme_stylebox_override("panel", style)
	add_child(content)
	hide()


func show_riders(peers: Array, imperial: bool) -> void:
	_title.text = tr("Riders", "In-ride group panel")
	visible = not peers.is_empty()
	var identities: Array[String] = []
	for peer: Dictionary in peers:
		identities.append(str(peer["id"]) + ":" + str(peer["generation"]) + ":" + str(peer["name"]))
	var key: String = str(identities)
	if key != _key:
		_key = key
		_labels.clear()
		_efforts.clear()
		_per_kg.clear()
		_speaking.clear()
		for child: Node in _rows.get_children():
			_rows.remove_child(child)
			child.queue_free()
		for peer: Dictionary in peers:
			var id: int = peer["id"]
			var rider_name: String = peer["name"]
			var heading: HBoxContainer = HBoxContainer.new()
			heading.add_theme_constant_override("separation", 6)
			heading.size_flags_horizontal = Control.SIZE_EXPAND_FILL
			var marker: ColorRect = ColorRect.new()
			marker.color = UiTheme.group_color(id)
			marker.custom_minimum_size = Vector2(3, 14)
			marker.size_flags_vertical = Control.SIZE_SHRINK_CENTER
			heading.add_child(marker)
			var label: Label = Label.new()
			label.text = rider_name
			label.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
			label.text_overrun_behavior = TextServer.OVERRUN_TRIM_ELLIPSIS
			label.tooltip_text = rider_name
			label.custom_minimum_size.x = 50
			label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
			label.add_theme_font_size_override("font_size", 14)
			heading.add_child(label)
			_speaking[id] = speaking_icon()
			heading.add_child(_speaking[id])
			_rows.add_child(heading)
			_efforts[id] = _column(43)
			_per_kg[id] = _column(63)
			_labels[id] = _column(80)
	for peer: Dictionary in peers:
		var id: int = peer["id"]
		var speaking: bool = peer.get("speaking", false)
		show_speaking(_speaking[id], speaking)
		var watts: float = peer.get("power_w", 0.0)
		var per_kg: float = peer.get("watts_per_kg", 0.0)
		_efforts[id].text = "%dW" % roundi(watts)
		_per_kg[id].text = "%.1f W/kg" % per_kg
		var gap: float = peer["gap_m"]
		var distance: String = (
			tr("%sft") % roundi(absf(gap) / HudPanel.METERS_PER_FOOT)
			if imperial
			else tr("%sm") % roundi(absf(gap))
		)
		_labels[id].text = tr("%s ahead") % distance if gap >= 0 else tr("%s behind") % distance


func _column(width: float) -> Label:
	var label: Label = Label.new()
	label.custom_minimum_size.x = width
	label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	label.add_theme_font_size_override("font_size", 13)
	_rows.add_child(label)
	return label


## The small speaker beside a rider's name while they are heard speaking. Hidden, it keeps
## its room, so names do not shift as riders start and stop talking.
static func speaking_icon() -> TextureRect:
	var icon: TextureRect = TextureRect.new()
	icon.texture = UiIcons.texture("speaking", 16, UiTheme.SUCCESS)
	icon.stretch_mode = TextureRect.STRETCH_KEEP_CENTERED
	icon.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	show_speaking(icon, false)
	return icon


static func show_speaking(icon: TextureRect, speaking: bool) -> void:
	icon.modulate.a = 1.0 if speaking else 0.0


## A rider's power and power per kilogram, e.g. "250 W · 3.4 W/kg".
static func effort(peer: Dictionary) -> String:
	var watts: float = peer.get("power_w", 0.0)
	var per_kg: float = peer.get("watts_per_kg", 0.0)
	return "%d W · %.1f W/kg" % [roundi(watts), per_kg]

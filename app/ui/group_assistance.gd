class_name GroupAssistance
extends PanelContainer
## Compact label/bar rows for applied assistance (R208). Rust supplies normalized strengths.

var _drafting: float = 0.0
var _together: float = 0.0
var _draft_label: Label = Label.new()
var _band_label: Label = Label.new()
var _draft_bar: Control = Control.new()
var _band_bar: Control = Control.new()
var _draft: HBoxContainer
var _band: HBoxContainer


func _init() -> void:
	mouse_filter = Control.MOUSE_FILTER_IGNORE
	custom_minimum_size.x = 216.0
	var style: StyleBoxFlat = UiTheme.bar()
	style.content_margin_left = 12.0
	style.content_margin_right = 12.0
	style.content_margin_top = 8.0
	style.content_margin_bottom = 8.0
	add_theme_stylebox_override("panel", style)
	var rows: VBoxContainer = VBoxContainer.new()
	rows.mouse_filter = Control.MOUSE_FILTER_IGNORE
	rows.add_theme_constant_override("separation", 3)
	_draft = _row(_draft_label, _draft_bar)
	_band = _row(_band_label, _band_bar)
	rows.add_child(_draft)
	rows.add_child(_band)
	add_child(rows)
	_draft_bar.draw.connect(func() -> void: _draw_bar(_draft_bar, _drafting, false))
	_band_bar.draw.connect(func() -> void: _draw_bar(_band_bar, _together, true))
	show_feedback({})


## Only active effects occupy space; both remain visible when drafting accompanies braking.
func show_feedback(feedback: Dictionary) -> void:
	_drafting = feedback.get("drafting", 0.0)
	_together = feedback.get("together", 0.0)
	_draft.visible = _drafting > 0.0
	_band.visible = _together != 0.0
	visible = _draft.visible or _band.visible
	_draft_label.text = tr("Drafting") + ":"
	_band_label.text = (tr("Support") if _together > 0.0 else tr("Braking")) + ":"
	_draft_bar.queue_redraw()
	_band_bar.queue_redraw()


func _row(label: Label, bar: Control) -> HBoxContainer:
	var row: HBoxContainer = HBoxContainer.new()
	row.mouse_filter = Control.MOUSE_FILTER_IGNORE
	row.add_theme_constant_override("separation", 8)
	label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	label.custom_minimum_size.x = 104.0
	label.add_theme_font_size_override("font_size", 13)
	row.add_child(label)
	bar.mouse_filter = Control.MOUSE_FILTER_IGNORE
	bar.custom_minimum_size = Vector2(80.0, 12.0)
	bar.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	bar.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	row.add_child(bar)
	return row


func _draw_bar(bar: Control, strength: float, centred: bool) -> void:
	var track: Rect2 = Rect2(0.0, 3.0, bar.size.x, 6.0)
	bar.draw_rect(track, UiTheme.SURFACE)
	var start: float = bar.size.x * 0.5 if centred else 0.0
	var length: float = strength * (bar.size.x * 0.5 if centred else bar.size.x)
	var color: Color = UiTheme.WARNING if strength < 0.0 else UiTheme.ACCENT
	var fill: Rect2 = Rect2(start + minf(length, 0.0), 3.0, absf(length), 6.0)
	bar.draw_rect(fill, color)
	if centred:
		bar.draw_line(Vector2(start, 0.0), Vector2(start, 12.0), UiTheme.MUTED, 1.0)

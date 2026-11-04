class_name GroupAssistance
extends HBoxContainer
## Applied local assistance (R208). Rust supplies normalized strengths; these bars only draw them.

var _drafting: float = 0.0
var _together: float = 0.0
var _draft_label: Label = Label.new()
var _band_label: Label = Label.new()
var _draft_bar: Control = Control.new()
var _band_bar: Control = Control.new()
var _draft: PanelContainer
var _band: PanelContainer


func _init() -> void:
	mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_theme_constant_override("separation", 10)
	_draft = _card(_draft_label, _draft_bar)
	_band = _card(_band_label, _band_bar)
	_draft_bar.draw.connect(func() -> void: _draw_bar(_draft_bar, _drafting, false))
	_band_bar.draw.connect(func() -> void: _draw_bar(_band_bar, _together, true))
	minimum_size_changed.connect(_fit)
	show_feedback({})


## Only active effects occupy space; both remain visible when drafting accompanies braking.
func show_feedback(feedback: Dictionary) -> void:
	_drafting = feedback.get("drafting", 0.0)
	_together = feedback.get("together", 0.0)
	_draft.visible = _drafting > 0.0
	_band.visible = _together != 0.0
	visible = _draft.visible or _band.visible
	_draft_label.text = tr("Drafting")
	_band_label.text = tr("Support") if _together > 0.0 else tr("Braking")
	_draft_bar.queue_redraw()
	_band_bar.queue_redraw()
	_fit()


func _fit() -> void:
	reset_size()
	if is_inside_tree():
		position.x = (get_parent_area_size().x - size.x) * 0.5


func _card(label: Label, bar: Control) -> PanelContainer:
	var card: PanelContainer = PanelContainer.new()
	card.mouse_filter = Control.MOUSE_FILTER_IGNORE
	card.custom_minimum_size.x = 144.0
	card.add_theme_stylebox_override("panel", UiTheme.panel(0.88))
	var box: VBoxContainer = VBoxContainer.new()
	box.mouse_filter = Control.MOUSE_FILTER_IGNORE
	label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	label.add_theme_font_size_override("font_size", 15)
	box.add_child(label)
	bar.mouse_filter = Control.MOUSE_FILTER_IGNORE
	bar.custom_minimum_size = Vector2(112.0, 12.0)
	box.add_child(bar)
	card.add_child(box)
	add_child(card)
	return card


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

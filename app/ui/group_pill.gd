class_name GroupPill
extends RefCounted
## The readiness pills of the group list (R196): green when ready, quiet while not.


## A friend's readiness, `width` wide.
static func make(ready: bool, width: float) -> PanelContainer:
	var pill: PanelContainer = PanelContainer.new()
	pill.custom_minimum_size.x = width
	pill.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	var color: Color = UiTheme.SUCCESS if ready else UiTheme.MUTED
	pill.add_theme_stylebox_override("panel", box(color, ready))
	var word: Label = Label.new()
	word.text = (
		TranslationServer.translate("Ready") if ready else TranslationServer.translate("Not ready")
	)
	word.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	word.add_theme_font_size_override("font_size", 14)
	word.add_theme_color_override("font_color", color)
	pill.add_child(word)
	return pill


## A pill's shape in `color`, `strong` for one that stands out.
static func box(color: Color, strong: bool) -> StyleBoxFlat:
	var pill: StyleBoxFlat = StyleBoxFlat.new()
	pill.bg_color = Color(color, 0.18 if strong else 0.08)
	pill.border_color = Color(color, 0.6 if strong else 0.25)
	pill.set_border_width_all(1)
	pill.set_corner_radius_all(15)
	pill.content_margin_left = 12
	pill.content_margin_right = 12
	pill.content_margin_top = 6
	pill.content_margin_bottom = 6
	pill.anti_aliasing = true
	return pill

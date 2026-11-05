class_name GroupRiderNameplate
extends PanelContainer
## Native UI keeps nameplates sharp and white, independent of the world's post-processing.

const FONT_SIZE: int = 15
const TEXT_WIDTH: float = 160.0
const POINTER_SIZE: Vector2 = Vector2(8.0, 4.0)
const OCCLUSION: Shader = preload("res://shaders/nameplate_occlusion.gdshader")

static var _theme: Theme

var text: String:
	get:
		return _label.text

var _accent: Color
var _source_name: String = ""
var _label: Label = Label.new()
var _pointer: Polygon2D = Polygon2D.new()
var _font: FontVariation = FontVariation.new()
var _occlusion: ShaderMaterial = ShaderMaterial.new()
var _visibility_texture: Texture2D


func _init(accent: Color) -> void:
	_accent = accent


func _ready() -> void:
	if _theme == null:
		_theme = UiTheme.build()
	theme = _theme
	mouse_filter = Control.MOUSE_FILTER_IGNORE
	var style: StyleBoxFlat = UiTheme.panel(0.94)
	style.set_corner_radius_all(7)
	style.set_content_margin_all(4.0)
	style.content_margin_left = 10.0
	style.content_margin_right = 10.0
	style.set_border_width_all(0)
	style.border_width_left = 3
	style.border_color = _accent
	style.shadow_size = 3
	style.shadow_offset = Vector2(0, 1)
	add_theme_stylebox_override("panel", style)
	_pointer.color = style.bg_color
	_pointer.antialiased = true
	_pointer.use_parent_material = true
	add_child(_pointer)
	resized.connect(_place_pointer)
	_place_pointer()
	_font.base_font = ThemeDB.fallback_font
	_font.variation_embolden = 0.65
	_label.add_theme_font_override("font", _font)
	_label.add_theme_font_size_override("font_size", FONT_SIZE)
	_label.add_theme_color_override("font_color", UiTheme.TEXT)
	_label.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_label.use_parent_material = true
	add_child(_label)
	_occlusion.shader = OCCLUSION


func _place_pointer() -> void:
	var base: Vector2 = Vector2(size.x * 0.5, size.y)
	_pointer.polygon = PackedVector2Array(
		[
			base + Vector2(-POINTER_SIZE.x * 0.5, 0.0),
			base + Vector2(POINTER_SIZE.x * 0.5, 0.0),
			base + Vector2(0.0, POINTER_SIZE.y),
		]
	)


func show_name(full_name: String) -> void:
	if _source_name != full_name:
		_source_name = full_name
		_label.text = _short_name()
		reset_size()


## Shared GPU visibility uses a single depth sample per badge, without softening UI text.
func set_occlusion(visibility_texture: Texture2D, slot: int) -> void:
	if visibility_texture != _visibility_texture:
		_visibility_texture = visibility_texture
		_occlusion.set_shader_parameter("visibility_mask", visibility_texture)
		material = _occlusion if visibility_texture != null else null
	_occlusion.set_shader_parameter("slot", (float(slot) + 0.5) / GroupNameplateVisibility.SLOTS)


func _short_name() -> String:
	var name_text: String = _source_name
	if _font.get_string_size(name_text, HORIZONTAL_ALIGNMENT_LEFT, -1, FONT_SIZE).x <= TEXT_WIDTH:
		return name_text
	while not name_text.is_empty():
		name_text = name_text.left(-1)
		if (
			_font.get_string_size(name_text + "…", HORIZONTAL_ALIGNMENT_LEFT, -1, FONT_SIZE).x
			<= TEXT_WIDTH
		):
			return name_text + "…"
	return "…"

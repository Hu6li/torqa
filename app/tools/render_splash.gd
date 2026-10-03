extends SceneTree
## Renders the boot splash PNG from the brand lockup SVG, because Godot's boot splash only accepts
## PNG. Run via scripts/render-splash.sh after changing the logo.

const SOURCE: String = "../docs/brand/torqa-lockup-dark.svg"
const TARGET: String = "res://assets/brand/torqa-splash.png"
## 16:9 like the default window; large enough to stay sharp on Retina and 1440p screens.
const SIZE: Vector2i = Vector2i(2560, 1440)
## The splash is scaled to fit the window, so this keeps the lockup the same share of any screen.
const LOGO_HEIGHT: float = 0.4


func _initialize() -> void:
	quit(0 if _render() == OK else 1)


func _render() -> Error:
	var svg: String = FileAccess.get_file_as_string(
		ProjectSettings.globalize_path("res://") + SOURCE
	)
	if svg.is_empty():
		push_error("cannot read %s" % SOURCE)
		return ERR_FILE_NOT_FOUND
	var probe: Image = Image.new()
	var err: Error = probe.load_svg_from_string(svg)
	if err != OK:
		return err
	var logo: Image = Image.new()
	err = logo.load_svg_from_string(svg, SIZE.y * LOGO_HEIGHT / probe.get_height())
	if err != OK:
		return err
	logo.convert(Image.FORMAT_RGBA8)
	var splash: Image = Image.create_empty(SIZE.x, SIZE.y, false, Image.FORMAT_RGBA8)
	splash.blit_rect(logo, Rect2i(Vector2i.ZERO, logo.get_size()), (SIZE - logo.get_size()) / 2)
	err = splash.save_png(ProjectSettings.globalize_path(TARGET))
	if err == OK:
		print("saved %s (%dx%d)" % [TARGET, SIZE.x, SIZE.y])
	return err

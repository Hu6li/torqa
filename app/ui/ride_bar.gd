class_name RideBar
extends PanelContainer
## The ride view's controls in one slim bar at the bottom left (#189): icon buttons with
## tooltips for the settings, the overlay, full screen and pausing, and the simulation's
## speeds when the ride is simulated. A chevron folds the bar away to its corner and back;
## the keys work either way.

## The settings were asked for (S).
signal settings_requested
## The overlay was asked for (O).
signal overlay_requested
## Full screen or a window was asked for (F11, #376).
signal fullscreen_requested
## The ride should pause, or go on (P).
signal pause_requested
## The way back was asked for after a ride recorded nothing.
signal back_requested
## A simulation speed was chosen.
signal speed_chosen(scale: float)
## The bar was folded away, or unfolded.
signal folded_changed(folded: bool)

const ICON: int = 20
const TIME_SCALES: Array[float] = [1.0, 2.0, 5.0, 10.0, 20.0]

## Folded away to its corner: only the chevron shows.
var folded: bool = false:
	set(value):
		folded = value
		_tools.visible = not folded
		_fold.icon = UiIcons.texture("unfold" if folded else "fold", ICON)
		_fold.tooltip_text = tr("Show the controls") if folded else tr("Fold the controls away")

var _fold: Button = Button.new()
var _tools: HBoxContainer = HBoxContainer.new()
var _settings: Button = Button.new()
var _overlay: Button = Button.new()
var _fullscreen: Button = Button.new()
var _fullscreen_on: bool = false
var _pause: Button = Button.new()
var _back: Button = Button.new()
var _simulation: HBoxContainer = HBoxContainer.new()
var _speed_buttons: Array[Button] = []


func _init() -> void:
	mouse_filter = Control.MOUSE_FILTER_STOP
	# No parent container shrinks this corner-anchored panel when controls disappear.
	minimum_size_changed.connect(_fit_controls)
	add_theme_stylebox_override("panel", UiTheme.bar())
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 4)
	add_child(row)
	_icon_button(_fold, "fold", "")
	_fold.pressed.connect(_toggle_fold)
	row.add_child(_fold)
	_tools.add_theme_constant_override("separation", 4)
	row.add_child(_tools)
	_icon_button(
		_settings, "cog", tr("Camera, difficulty, weather, HUD; finish or abort (keyboard: S)")
	)
	_settings.pressed.connect(settings_requested.emit)
	_tools.add_child(_settings)
	_icon_button(
		_overlay, "overlay", tr("Only the HUD, on top of other windows, e.g. over a video (O)")
	)
	_overlay.pressed.connect(overlay_requested.emit)
	_tools.add_child(_overlay)
	_icon_button(_fullscreen, "fullscreen", tr("Full screen (F11)"))
	_fullscreen.pressed.connect(fullscreen_requested.emit)
	_tools.add_child(_fullscreen)
	_icon_button(_pause, "pause", tr("Pause the ride: the clock and the trainer wait (P)"))
	_pause.pressed.connect(pause_requested.emit)
	_tools.add_child(_pause)
	_icon_button(_back, "back", tr("Back"))
	_back.pressed.connect(back_requested.emit)
	_back.hide()
	_tools.add_child(_back)
	_simulation.add_theme_constant_override("separation", 4)
	_simulation.add_child(VSeparator.new())
	var caption: Label = UiTheme.caption(tr("Simulation"))
	caption.mouse_filter = Control.MOUSE_FILTER_STOP
	caption.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	caption.tooltip_text = tr(
		"Click the map or profile to jump · + / − speed · C: free camera, Shift + arrows or mouse to look"
	)
	_simulation.add_child(caption)
	var group: ButtonGroup = ButtonGroup.new()
	for scale: float in TIME_SCALES:
		var button: Button = Button.new()
		button.text = "%d×" % roundi(scale)
		button.toggle_mode = true
		button.button_group = group
		button.focus_mode = Control.FOCUS_NONE
		button.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		button.tooltip_text = tr("Simulation speed")
		button.pressed.connect(speed_chosen.emit.bind(scale))
		_simulation.add_child(button)
		_speed_buttons.append(button)
	_simulation.hide()
	_tools.add_child(_simulation)
	folded = false


func _ready() -> void:
	_show_fullscreen()


func _process(_delta: float) -> void:
	# A mode change need not resize the window, e.g. when it already fills the screen.
	if FullScreen.is_on(get_window()) != _fullscreen_on:
		_show_fullscreen()


func _notification(what: int) -> void:
	if what == NOTIFICATION_TRANSLATION_CHANGED and is_node_ready():
		_show_fullscreen()


func _show_fullscreen() -> void:
	var on: bool = FullScreen.is_on(get_window())
	_fullscreen_on = on
	_fullscreen.icon = UiIcons.texture("windowed" if on else "fullscreen", ICON)
	_fullscreen.tooltip_text = (
		tr("Exit full screen (Esc or F11)") if on else tr("Full screen (F11)")
	)


## Shows the simulation's speeds, or hides them.
func set_simulating(simulating: bool) -> void:
	_simulation.visible = simulating


## Marks the speed in effect.
func show_speed(scale: float) -> void:
	for i: int in range(_speed_buttons.size()):
		_speed_buttons[i].set_pressed_no_signal(is_equal_approx(TIME_SCALES[i], scale))


## Shows the pause button while a ride is under way, or hides it.
func show_pause(shown: bool) -> void:
	_pause.visible = shown


## Shows the ride as paused (play to go on) or as going (pause).
func show_paused(paused: bool) -> void:
	_pause.icon = UiIcons.texture("play" if paused else "pause", ICON)
	_pause.tooltip_text = (
		tr("Resume") if paused else tr("Pause the ride: the clock and the trainer wait (P)")
	)


## Shows the settings button, or hides it (once the ride is over).
func show_settings(shown: bool) -> void:
	_settings.visible = shown


## Shows the way back when a ride recorded nothing and has no summary.
func show_back(shown: bool) -> void:
	_back.visible = shown


func _fit_controls() -> void:
	var bottom: float = position.y + size.y
	reset_size()
	position.y = bottom - size.y


func _toggle_fold() -> void:
	folded = not folded
	folded_changed.emit(folded)


func _icon_button(button: Button, icon: String, tooltip: String) -> void:
	button.icon = UiIcons.texture(icon, ICON)
	button.tooltip_text = tooltip
	button.focus_mode = Control.FOCUS_NONE
	button.add_theme_stylebox_override("normal", UiTheme.icon_button(Color.TRANSPARENT))
	button.add_theme_stylebox_override("hover", UiTheme.icon_button(Color(1, 1, 1, 0.13)))
	button.add_theme_stylebox_override("pressed", UiTheme.icon_button(Color(UiTheme.ACCENT, 0.85)))

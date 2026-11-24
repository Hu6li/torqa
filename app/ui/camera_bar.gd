class_name CameraBar
extends PanelContainer
## Small buttons above the ride bar for the camera (R276): the next camera, round the rider to
## their left or right, nearer and further. They do what C and Shift + arrows do, for riders
## with a mouse or a touch screen rather than a keyboard at hand.

## The next camera was asked for (C).
signal camera_requested
## The camera should go `step` sides round the rider, positive towards their right
## (Shift + left / right).
signal orbit_requested(step: int)
## The camera should come `step` distances nearer (negative) or go further (Shift + up / down).
signal distance_requested(step: int)

const ICON: int = 16


func _init() -> void:
	mouse_filter = Control.MOUSE_FILTER_STOP
	add_theme_stylebox_override("panel", UiTheme.bar())
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 2)
	add_child(row)
	_add(row, "camera", tr("Next camera (C)"), camera_requested.emit)
	_add(
		row, "orbit_left", tr("Camera round to the left (Shift + ←)"), orbit_requested.emit.bind(-1)
	)
	_add(
		row,
		"orbit_right",
		tr("Camera round to the right (Shift + →)"),
		orbit_requested.emit.bind(1)
	)
	_add(row, "nearer", tr("Camera nearer (Shift + ↑)"), distance_requested.emit.bind(-1))
	_add(row, "further", tr("Camera further away (Shift + ↓)"), distance_requested.emit.bind(1))


static func _add(row: HBoxContainer, icon: String, tooltip: String, action: Callable) -> void:
	var button: Button = Button.new()
	RideBar.icon_button(button, icon, tooltip, ICON)
	# Tighter than the bar's buttons, so the row stays small.
	for state: String in ["normal", "hover", "pressed"]:
		(button.get_theme_stylebox(state) as StyleBoxFlat).set_content_margin_all(5.0)
	button.pressed.connect(action)
	row.add_child(button)

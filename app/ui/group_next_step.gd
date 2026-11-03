class_name GroupNextStep
extends VBoxContainer
## What the rider does next in a group ride (R196): one main button, always the same size,
## following the steps: connect the trainer, then start the ride as organizer or join friends
## already riding. Readiness is the pill beside the rider's own name. A line says what the
## group is waiting for, with how far the ride is prepared.

## The rider wants to connect their trainer.
signal devices_requested

var _orqwa: OrqwaApp
var _line: Label = Label.new()
## Turns while ORQWA connects, receives or prepares the ride and the rider can only wait (#420).
var _working: Spinner = Spinner.new(18.0)
var _progress: ProgressBar = ProgressBar.new()
var _bytes: Label = Label.new()
var _main: Button = Button.new()
var _retry: Button = Button.new()
var _reconnect: Button = Button.new()
## What the main button does now: `devices`, `start` or `enter`.
var _action: String = ""


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa


func _ready() -> void:
	add_theme_constant_override("separation", 10)
	var line_row: HBoxContainer = HBoxContainer.new()
	line_row.add_theme_constant_override("separation", 10)
	_working.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	line_row.add_child(_working)
	_line.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_line.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_line.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	line_row.add_child(_line)
	add_child(line_row)
	_progress.show_percentage = false
	_progress.custom_minimum_size.y = 6
	add_child(_progress)
	_bytes.add_theme_color_override("font_color", UiTheme.MUTED)
	add_child(_bytes)
	_main.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_main.pressed.connect(_act)
	_retry.text = tr("Retry download")
	_retry.pressed.connect(func() -> void: _orqwa.group_retry())
	_reconnect.text = tr("Reconnect now")
	_reconnect.pressed.connect(func() -> void: _orqwa.group_reconnect())
	# Every step's button the same size, whichever is next.
	for button: Button in [_main, _retry, _reconnect]:
		button.custom_minimum_size.y = 48
		button.add_theme_font_size_override("font_size", 17)
		add_child(button)


func _act() -> void:
	match _action:
		"devices":
			devices_requested.emit()
		"start":
			_orqwa.group_start()
		"enter":
			_orqwa.group_enter()


## Shows the next step of `status` (`OrqwaApp.group_status()`), with or without a `trainer`
## connected; `organizer` names who starts the ride, `percent` how far this rider's world is
## built, and `devices` whether the trainer may be connected from here.
func show_status(
	status: Dictionary, trainer: bool, organizer: String, percent: int, devices: bool
) -> void:
	var phase: String = status.get("phase", "idle")
	var host: bool = status.get("host", false)
	var late: bool = status.get("late", false)
	var underway: bool = phase in ["countdown", "riding", "ended"]
	var can_retry: bool = status.get("can_retry", false)
	_show_progress(status, percent)
	_action = ""
	if late and not underway:
		_action = "enter" if trainer else "devices"
	elif not underway and not trainer:
		_action = "devices"
	elif not underway and host:
		_action = "start"
	if _action == "devices" and not devices:
		_action = ""
	var labels: Dictionary[String, String] = {
		"devices": tr("Connect your trainer"),
		"start": tr("Start group ride"),
		"enter": tr("Join ride now"),
	}
	_main.visible = not _action.is_empty()
	_main.text = labels.get(_action, "")
	var entering: bool = status.get("entering", false)
	_main.disabled = (
		entering
		or (_action == "start" and not status.get("can_start", false))
		or (_action == "enter" and not status.get("can_enter", false))
	)
	_retry.visible = can_retry
	var reconnect: float = status.get("reconnect_s", 0.0)
	_reconnect.visible = reconnect > 0.0
	_working.visible = (
		(phase in ["connecting", "downloading", "preparing"] and not can_retry) or reconnect > 0.0
	)
	_show_line(status, organizer, reconnect)
	# Nothing to do and nothing to wait for, e.g. while riding: the block makes no room.
	visible = (
		_main.visible or _line.visible or _progress.visible or _retry.visible or _reconnect.visible
	)


func _show_progress(status: Dictionary, percent: int) -> void:
	var phase: String = status.get("phase", "idle")
	var received: float = status.get("received", 0)
	var total: float = status.get("total", 0)
	_progress.visible = phase == "downloading" and total > 0
	_progress.max_value = maxf(1, total)
	_progress.value = received
	_bytes.text = tr("%.1f / %.1f MiB") % [received / 1048576, total / 1048576]
	# Building the course's world takes longest; the group hears how far it has got too.
	if phase == "preparing" and percent > 0:
		_progress.visible = true
		_progress.max_value = 100
		_progress.value = percent
		_bytes.text = tr("Building the world · %d %%") % percent
	_bytes.visible = _progress.visible


## The one line worth reading: what the group waits for, never what the main button says.
func _show_line(status: Dictionary, organizer: String, reconnect: float) -> void:
	var phase: String = status.get("phase", "idle")
	var host: bool = status.get("host", false)
	var name: String = status.get("name", "")
	var prepared: bool = status.get("prepared", false)
	var late: bool = status.get("late", false)
	var ready: bool = status.get("ready", false)
	var underway: bool = phase in ["countdown", "riding", "ended"]
	var text: String = ""
	var good: bool = false
	if not name.is_empty() and not prepared and not underway:
		text = tr("Getting your ride ready…")
	if late and prepared and not underway:
		text = tr("Join near the organizer at the current workout step. Your recording starts now.")
	elif ready and host and not underway:
		var waiting: PackedStringArray = []
		for rider: Dictionary in status.get("roster", []):
			if not rider.get("ready", false) and not rider.get("me", false):
				var rider_name: String = rider.get("name", "")
				waiting.append(rider_name)
		good = waiting.is_empty()
		text = tr("Everyone is ready") if good else tr("Waiting for %s") % ", ".join(waiting)
	elif ready and not underway:
		good = true
		text = tr("You're ready. %s starts the ride.") % organizer
	if status.get("can_retry", false):
		text = tr("Download stopped. Try again when you're ready.")
	if status.get("entering", false):
		text = tr("Joining your friends…")
	# The connection to the organizer was lost and comes back by itself (#434).
	if reconnect > 0.0:
		text = (
			tr("Trying for another %s. You keep your place in the group.")
			% ("%d:%02d" % [floori(reconnect / 60.0), floori(reconnect) % 60])
		)
	# The countdown replaces readiness here so no new row shifts the course preview (R202).
	if phase == "countdown" and not late and reconnect <= 0.0:
		var countdown: float = status.get("countdown_s", 0.0)
		text = tr("Starting in %s seconds…") % ceili(countdown)
		good = false
	_line.text = text
	_line.visible = not text.is_empty()
	_line.add_theme_color_override("font_color", UiTheme.SUCCESS if good else UiTheme.MUTED)

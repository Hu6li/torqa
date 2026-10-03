extends Control
## Root scene: owns the Torqa node and switches between the setup, ride and history screens.

@onready var _torqa: TorqaApp = $Torqa
@onready var _world: RideWorld = $World
@onready var _setup: SetupScreen = $SetupScreen
@onready var _ride: RideScreen = $RideScreen
@onready var _history: HistoryScreen = $HistoryScreen


func _ready() -> void:
	print("Torqa %s" % TorqaCore.version())
	theme = UiTheme.build()
	_world.bind(_torqa)
	_setup.bind(_torqa)
	_ride.bind(_torqa, _world)
	_history.bind(_torqa)
	_setup.ride_started.connect(_on_ride_started)
	_setup.history_requested.connect(_show_history)
	_ride.closed.connect(_on_ride_closed)
	_ride.summary_requested.connect(_show_summary)
	_history.closed.connect(_on_history_closed)


func _on_ride_started() -> void:
	_setup.hide()
	var options: Dictionary = _setup.ride_options()
	_world.apply_options(options)
	_world.reset_view()
	_world.show()
	_ride.begin(options)
	_ride.show()


func _on_ride_closed() -> void:
	_ride.hide()
	_world.hide()
	_setup.show()


func _show_history() -> void:
	_ride.hide()
	_world.hide()
	_setup.hide()
	_history.open()
	_history.show()


## The summary of the ride just saved (R42); closing it returns to the start.
func _show_summary(path: String) -> void:
	_ride.hide()
	_world.hide()
	_setup.hide()
	_history.open_summary(path)
	_history.show()


func _on_history_closed() -> void:
	_history.hide()
	_setup.show()

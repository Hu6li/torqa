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
	_ride.summary_requested.connect(_show_history)
	_history.closed.connect(_on_history_closed)


func _on_ride_started() -> void:
	_setup.hide()
	var conditions: Dictionary = _setup.conditions()
	var time: String = conditions["time"]
	var weather: String = conditions["weather"]
	_world.apply_conditions(time, weather)
	_world.reset_view()
	_world.show()
	_ride.begin()
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


func _on_history_closed() -> void:
	_history.hide()
	_setup.show()

extends Control
## Root scene: owns the Torqa node and switches between the setup and ride screens.

@onready var _torqa: TorqaApp = $Torqa
@onready var _world: RideWorld = $World
@onready var _setup: SetupScreen = $SetupScreen
@onready var _ride: RideScreen = $RideScreen


func _ready() -> void:
	print("Torqa %s" % TorqaCore.version())
	_world.bind(_torqa)
	_setup.bind(_torqa)
	_ride.bind(_torqa, _world)
	_setup.ride_started.connect(_on_ride_started)
	_ride.closed.connect(_on_ride_closed)


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

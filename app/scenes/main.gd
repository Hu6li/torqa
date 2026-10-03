extends Control
## Root scene: owns the Torqa node and switches between the start page, the ride and the ride
## summary.

@onready var _torqa: TorqaApp = $Torqa
@onready var _world: RideWorld = $World
@onready var _start: StartPage = $StartPage
@onready var _ride: RideScreen = $RideScreen
@onready var _summary: HistoryScreen = $HistoryScreen


func _ready() -> void:
	print("Torqa %s" % TorqaCore.version())
	theme = UiTheme.build()
	_world.bind(_torqa)
	_start.bind(_torqa)
	_ride.bind(_torqa, _world)
	_summary.bind(_torqa)
	_start.ride_started.connect(_on_ride_started)
	_ride.closed.connect(_back_home)
	_ride.summary_requested.connect(_show_summary)
	_summary.closed.connect(_back_home)


func _on_ride_started(options: Dictionary) -> void:
	_start.hide()
	_world.apply_options(options)
	_world.reset_view()
	_world.show()
	_ride.begin(options)
	_ride.show()


## The summary of the ride just saved (R42); closing it returns to the start page.
func _show_summary(path: String) -> void:
	_ride.hide()
	_world.hide()
	_summary.open_summary(path)
	_summary.show()


func _back_home() -> void:
	_ride.hide()
	_world.hide()
	_summary.hide()
	_start.refresh()
	_start.show()

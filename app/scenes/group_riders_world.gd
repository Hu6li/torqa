class_name GroupRidersWorld
extends Node3D
## Owns current group avatars independently of course meshes and the local riding camera.

var _avatars: Dictionary[int, GroupRiderAvatar] = {}
var _visibility: GroupNameplateVisibility
var _visibility_camera: Camera3D
var _previous_compositor: Compositor
@onready var _world: Node3D = get_parent() as Node3D


func _process(delta: float) -> void:
	if _world == null:
		return
	var app: OrqwaApp = _world.get("_orqwa")
	var state: Dictionary = {}
	if app != null and _world.visible:
		state = app.ride_state()
	var peers: Array = state.get("peers", [])
	show_riders(peers if state.has("rider_x") else [], delta)


func show_riders(peers: Array, delta: float) -> void:
	var current: Dictionary[int, bool] = {}
	for peer: Dictionary in peers:
		if not peer.has("rider_x"):
			continue
		var id: int = peer["id"]
		var generation: String = peer["generation"]
		current[id] = true
		var avatar: GroupRiderAvatar = _avatars.get(id)
		if avatar != null and avatar.generation != generation:
			remove_child(avatar)
			avatar.queue_free()
			avatar = null
		if avatar == null:
			var look: String = peer.get("avatar", "")
			avatar = GroupRiderAvatar.new(id, generation, look)
			_avatars[id] = avatar
			add_child(avatar)
		avatar.show_state(peer, delta)
	for id: int in _avatars.keys():
		if not current.has(id):
			var avatar: GroupRiderAvatar = _avatars[id]
			remove_child(avatar)
			avatar.queue_free()
			_avatars.erase(id)
	_place_names()


func _place_names() -> void:
	var camera: Camera3D = get_viewport().get_camera_3d()
	var ordered: Array[GroupRiderAvatar] = []
	ordered.assign(_avatars.values())
	if camera != null:
		ordered.sort_custom(
			func(a: GroupRiderAvatar, b: GroupRiderAvatar) -> bool:
				return (
					a.global_position.distance_squared_to(camera.global_position)
					< b.global_position.distance_squared_to(camera.global_position)
				)
		)
	var occupied: Array[Rect2] = []
	if camera != null and not ordered.is_empty():
		_attach_visibility(camera)
	var texture: Texture2D = _visibility.visibility_texture() if _visibility != null else null
	var samples: PackedFloat32Array = PackedFloat32Array()
	samples.resize(GroupNameplateVisibility.SLOTS * 4)
	for index: int in range(mini(ordered.size(), GroupNameplateVisibility.SLOTS)):
		var point: Vector4 = ordered[index].place_name(camera, occupied, texture, index)
		for component: int in range(4):
			samples[index * 4 + component] = point[component]
	if _visibility != null:
		_visibility.enabled = not ordered.is_empty()
		_visibility.update_samples(samples)


func _attach_visibility(camera: Camera3D) -> void:
	if _visibility_camera == camera or RenderingServer.get_rendering_device() == null:
		return
	_detach_visibility()
	_visibility_camera = camera
	_previous_compositor = camera.compositor
	var compositor: Compositor = Compositor.new()
	var effects: Array[CompositorEffect] = []
	if _previous_compositor != null:
		effects.assign(_previous_compositor.compositor_effects)
	_visibility = GroupNameplateVisibility.new()
	effects.append(_visibility)
	compositor.compositor_effects = effects
	camera.compositor = compositor


func _exit_tree() -> void:
	_detach_visibility()


func _detach_visibility() -> void:
	if is_instance_valid(_visibility_camera):
		_visibility_camera.compositor = _previous_compositor
	_visibility = null
	_visibility_camera = null
	_previous_compositor = null

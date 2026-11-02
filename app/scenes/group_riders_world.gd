class_name GroupRidersWorld
extends Node3D
## Owns current group avatars independently of course meshes and the local riding camera.

var _avatars: Dictionary[int, GroupRiderAvatar] = {}
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

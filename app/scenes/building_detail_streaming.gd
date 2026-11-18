class_name BuildingDetailStreaming
extends RefCounted
## Keep only the nearby façade additions in memory, including a margin for moving cameras.

const STEP: float = 64.0
const LOAD_RANGE: float = BuildingDetails.RANGE + BuildingDetails.FADE + STEP * 2.0

var _entries: Array[Dictionary] = []


func reset() -> void:
	for entry: Dictionary in _entries:
		var detail: Node3D = entry["detail"]
		if detail != null:
			detail.get_parent().remove_child(detail)
			detail.queue_free()
	_entries.clear()


func register(index: int, node: Node3D, bounds: AABB) -> void:
	unregister(index)
	var corner: Vector3 = node.position + bounds.position
	_entries.append(
		{
			"index": index,
			"parent": node,
			"bounds": Rect2(Vector2(corner.x, corner.z), Vector2(bounds.size.x, bounds.size.z)),
			"focus": Vector2(INF, INF),
			"detail": null
		}
	)


## Forgets a chunk the streaming let go of; its additions go with the chunk's node.
func unregister(index: int) -> void:
	for at: int in range(_entries.size() - 1, -1, -1):
		var entry: Dictionary = _entries[at]
		if entry["index"] == index:
			var detail: Node3D = entry["detail"]
			if detail != null and is_instance_valid(detail):
				detail.get_parent().remove_child(detail)
				detail.queue_free()
			_entries.remove_at(at)


func update(focus: Vector2, load_details: Callable) -> void:
	var built: bool = false
	for entry: Dictionary in _entries:
		var bounds: Rect2 = entry["bounds"]
		var detail: Node3D = entry["detail"]
		var previous: Vector2 = entry["focus"]
		if not bounds.grow(LOAD_RANGE).has_point(focus):
			if detail != null:
				detail.get_parent().remove_child(detail)
				detail.queue_free()
				entry["detail"] = null
				entry["focus"] = Vector2(INF, INF)
		elif not built and previous.distance_to(focus) >= STEP:
			var index: int = entry["index"]
			var arrays: Array = load_details.call(index, focus.x, focus.y, LOAD_RANGE)
			if detail != null:
				detail.get_parent().remove_child(detail)
				detail.queue_free()
			var parent: Node3D = entry["parent"]
			detail = Node3D.new()
			parent.add_child(detail)
			BuildingDetails.add_to(detail, arrays)
			entry["detail"] = detail
			entry["focus"] = focus
			built = true


func ready_at(focus: Vector2) -> bool:
	for entry: Dictionary in _entries:
		var bounds: Rect2 = entry["bounds"]
		var previous: Vector2 = entry["focus"]
		if bounds.grow(LOAD_RANGE).has_point(focus) and previous.distance_to(focus) >= STEP:
			return false
	return true

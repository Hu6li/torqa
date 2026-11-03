extends Node
## Enforces window bounds for current and future scenes, regardless of their popup method.


func _ready() -> void:
	get_tree().node_added.connect(_node_added)
	for node: Node in get_tree().root.find_children("*", "Window", true, false):
		_node_added(node)


func _node_added(node: Node) -> void:
	if not node is Window or node == get_tree().root:
		return
	var window: Window = node as Window
	if window.is_node_ready():
		WindowBounds.attach(window)
	else:
		window.ready.connect(func() -> void: WindowBounds.attach(window), CONNECT_ONE_SHOT)

"""Scoped Administration AX assertions shared by C31 and C39 native drivers.

The navigation rail and another expanded product are outside a product's
inventory/detail authority. Missing card/table/detail never satisfies a
negative action assertion. This module performs no UI or endpoint operation.
"""


def subtree(tree, node):
    if node is None:
        return []
    start = next(i for i, candidate in enumerate(tree) if candidate is node)
    end = next((i for i in range(start + 1, len(tree))
                if tree[i].get("depth", 0) <= node.get("depth", 0)), len(tree))
    return tree[start:end]


def product_nodes(tree, product):
    heading = next((n for n in tree if n.get("role") == "AXHeading"
                    and (n.get("label") or "").startswith(product + " ")), None)
    if heading is None:
        return []
    index = next(i for i, n in enumerate(tree) if n is heading)
    parent = next((n for n in reversed(tree[:index]) if n.get("role") == "AXGroup"
                   and n.get("depth", 0) < heading.get("depth", 0)), None)
    return subtree(tree, parent)


def product_header(tree, product):
    nodes = product_nodes(tree, product)
    heading = next((n for n in nodes if n.get("role") == "AXHeading"
                    and (n.get("label") or "").startswith(product + " ")), None)
    return next((n for n in nodes if n.get("role") == "AXButton"
                 and heading and n.get("label") == heading.get("label")), None)


def button(nodes, label):
    return next((n for n in nodes if n.get("role") == "AXButton" and n.get("label") == label), None)


def detail_nodes(tree, product):
    nodes = product_nodes(tree, product)
    for node in nodes:
        if node.get("subrole") == "AXLandmarkRegion":
            detail = subtree(tree, node)
            if button(detail, "Close details") is not None:
                return detail
    return []


def act_on_node(app, tree, node, action="press"):
    if node is None:
        raise ValueError("required scoped AX node is missing")
    role, label = node["role"], node["label"]
    matches = [n for n in tree if n.get("role") == role
               and (n.get("label") or "").startswith(label)]
    occurrence = next(i for i, candidate in enumerate(matches) if candidate is node)
    return app.ax(action, role, label, occurrence)


def gated_product(tree, product, gate):
    nodes = product_nodes(tree, product)
    header = product_header(tree, product)
    words = " ".join(str(n.get("label") or n.get("value") or "") for n in nodes)
    controls = [n for n in nodes if n.get("role") in
                ("AXButton", "AXRadioButton", "AXTab", "AXTextField", "AXTextArea", "AXCheckBox", "AXTable")]
    return bool(nodes and header and gate in words and len(controls) == 1 and controls[0] is header)


def gateway_key_state(tree, kind, require_detail=False):
    nodes = product_nodes(tree, "AI Gateway")
    detail = detail_nodes(tree, "AI Gateway")
    tabs = [n.get("label") for n in nodes if n.get("role") in ("AXRadioButton", "AXTab")]
    forbidden = sorted({n.get("label") for n in nodes
                        if n.get("role") in ("AXButton", "AXMenuItem", "AXLink")
                        and (n.get("label") or "").startswith(("Issue additional key", "Finish rotation"))})
    actions = [n.get("label") for n in detail if n.get("role") == "AXButton"]
    name = next((n.get("label") for n in detail if n.get("role") == "AXHeading"), None)
    words = " ".join(str(n.get("label") or n.get("value") or "") for n in detail)
    issues = [message for message in ("Reading details", "did not find this resource",
              "could not be read", "does not contain this item", "It is not shown") if message in words]
    if any(n.get("role") == "AXAlert" or n.get("subrole") == "AXApplicationAlert" for n in detail):
        issues.append("detail alert")
    expected = {f"{verb} {name}" for verb in ("Edit", "Rotate", "Remove")} | {"Close details"}
    success = bool(detail and name and set(actions) == expected and not issues)
    value = {"card_present": bool(nodes), "kind": kind,
             "table": any(n.get("role") == "AXTable" and n.get("label") == kind for n in nodes),
             "gateway_tabs": tabs, "no_limits": "Keys" in tabs and "Limits" not in tabs,
             "forbidden_actions": forbidden, "detail_open": bool(detail),
             "detail_actions": actions, "detail_name": name, "detail_issues": issues,
             "detail_success": success}
    value["ok"] = bool(nodes and value["table"] and value["no_limits"] and not forbidden
                       and (not require_detail or success))
    return value


def gateway_keys_complete(value):
    kinds = value.get("kinds", {})
    if set(kinds) != {"Service keys", "User keys"}:
        return False
    if not all(kinds[kind].get("details") for kind in ("Service keys", "User keys")):
        return False
    for kind, observed in kinds.items():
        state = observed.get("list", {})
        details = observed.get("details", [])
        expected = observed.get("row_labels")
        def complete(row, require_detail):
            tabs = row.get("gateway_tabs")
            mandatory = {"kind", "ok", "card_present", "table", "no_limits", "forbidden_actions",
                         "gateway_tabs", "detail_actions", "detail_open", "detail_name", "detail_issues", "detail_success"}
            expected = {f"{verb} {row.get('detail_name')}" for verb in ("Edit", "Rotate", "Remove")} | {"Close details"}
            return (mandatory <= set(row) and row.get("kind") == kind and row.get("ok") is True
                    and row.get("card_present") is True and row.get("table") is True
                    and row.get("no_limits") is True and row.get("forbidden_actions") == []
                    and isinstance(tabs, list) and "Keys" in tabs and "Limits" not in tabs
                    and isinstance(row.get("detail_actions"), list)
                    and row.get("detail_open") is require_detail
                    and (not require_detail or (row.get("detail_success") is True and row.get("detail_name")
                         and row.get("detail_issues") == [] and set(row["detail_actions"]) == expected)))
        if (not complete(state, False) or not isinstance(expected, list)
                or len(set(expected)) != len(expected) or len(details) != len(expected)):
            return False
        if any(row.get("row") != label or not complete(row, True)
               for row, label in zip(details, expected)):
            return False
    return True

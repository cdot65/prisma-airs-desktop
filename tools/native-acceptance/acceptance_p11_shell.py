"""P11 shell native observations; exact fixture process and keyboard only."""
import time
import acceptance_c29 as c29
import acceptance_c41 as c41
import acceptance_p11_setup as setup

PAGES = ('Home', 'Inbox', 'Test Cases', 'API Client', 'Administration', 'Settings', 'Harness', 'Academy')
ACTIONS = {'Home': ('Refresh',), 'Inbox': ('Refresh',), 'Test Cases': ('Load packs', 'New case'),
           'API Client': ('URL', 'Send'), 'Administration': ('Expand all', 'Collapse all'),
           'Settings': ('Appearance', 'Theme'), 'Harness': ('New investigation',), 'Academy': ('Learn', 'Open Runtime')}


def title(tree, label):
    return [n for n in tree if n.get('role') == 'AXHeading' and n.get('label') == label and n.get('value') == 1]


def main_region(tree):
    return next((n for n in tree if n.get('subrole') == 'AXLandmarkMain'), None)


def focus_visible(ctx, label):
    ctx.app.ax('focus', 'AXHeading', label)
    for _ in range(24):
        ctx.app.keycode(48)
        tree = ctx.app.tree()
        focused = ctx.app.focused(tree)
        if focused and any((focused.get('label') or '').startswith(name) for name in ACTIONS[label]):
            return c29.inside(c29.window_frame(ctx.app.pid), focused), focused
    return False, ctx.app.focused(ctx.app.tree())


def run(ctx):
    setup.switch(ctx, ctx.state['bootstrap_tenant'])
    layouts, artifacts, pictures = [], [], []
    ctx.navigate('Inbox')  # ensure the first Home entry is an actual navigation
    # One paired matrix: ordinary light and narrow/effective200% dark on
    # every changed destination. C49 independently exercises all4palettes.
    for theme, width, height, zoom in (('Light', 1440, 820, 0), ('Dark', 900, 600, 10)):
        ctx.app.menu('View', 'Actual Size')
        ctx.app.front()
        c41.size_window(ctx.app, ctx.app.pid, width, height)
        c29.appearance(ctx.app, theme)
        if zoom:
            c29.zoom(ctx.app, zoom)
        for page in PAGES:
            tree = ctx.navigate(page)
            tree, focused = ctx.app.wait_for(lambda t: bool((ctx.app.focused(t) or {}).get('label') == page), 8)
            headers = title(tree, page)
            main = main_region(tree)
            reachable, control = focus_visible(ctx, page)
            current = ctx.app.tree()
            name = 'shell-'+page.lower().replace(' ', '-')+'-'+theme.lower()+'-'+('200' if zoom else '100')
            artifacts.append(ctx.save(name, current))
            pictures.append(ctx.capture(name))
            layouts.append({'page': page, 'theme': theme, 'zoom': 200 if zoom else 100,
                            'requested': [width, height], 'geometry': c41.geometry(current),
                            'one_named_header_h1': len(headers) == 1, 'navigation_focus': focused,
                            'main': main, 'main_inside_window': c29.inside(c29.window_frame(ctx.app.pid), main),
                            'named_action_keyboard_visible': reachable, 'focused_action': control,
                            'academy_location': page != 'Academy' or ctx.app.has_text(current, 'Academy location')})
    matrix = ctx.save('shell-layout-measurements', layouts)
    ctx.record('C47-N01', {'all_destinations_have_one_named_header_h1': all(r['one_named_header_h1'] for r in layouts),
                          'navigation_focus_observed': all(r['navigation_focus'] for r in layouts),
                          'main_frames_measured_inside_window': all(r['main_inside_window'] for r in layouts),
                          'named_actions_keyboard_reachable': all(r['named_action_keyboard_visible'] for r in layouts),
                          'academy_hierarchy_retained': all(r['academy_location'] for r in layouts)}, [matrix, *artifacts], pictures)
    ctx.app.menu('View', 'Actual Size')
    c29.set_window_size(ctx.app.pid, 1440, 900)
    ctx.navigate('Home')
    before = main_region(ctx.app.tree())
    ctx.press_any('Show the calls')
    ctx.text('The calls')
    opened = ctx.app.tree()
    after = main_region(opened)
    same_width = bool(before and after and abs(before['size'][0]-after['size'][0]) <= 1)
    drawer_capture = ctx.capture('explain-overlay')
    ctx.press('AXButton', 'Refresh')
    control_live = ctx.app.node(ctx.app.tree(), 'AXButton', 'Close Explain') is not None
    ctx.navigate('Inbox')
    navigation_live = ctx.app.node(ctx.app.tree(), 'AXButton', 'Close Explain') is not None
    # Escape on the page belongs to the page, not the nonmodal drawer.
    ctx.app.ax('focus', 'AXHeading', 'Inbox')
    ctx.app.keycode(53)
    page_escape_kept = ctx.app.node(ctx.app.tree(), 'AXButton', 'Close Explain') is not None
    ctx.press('AXButton', 'Search or go to')
    ctx.text('Command palette')
    ctx.app.keycode(53)
    modal_priority = ctx.app.node(ctx.app.tree(), 'AXButton', 'Close Explain') is not None
    ctx.app.ax('focus', 'AXButton', 'Close Explain')
    ctx.app.keycode(53)
    tree, closed = ctx.app.wait_for(lambda t: ctx.app.node(t, 'AXButton', 'Close Explain') is None, 8)
    focus_back = (ctx.app.focused(tree) or {}).get('label') == 'Show the calls'
    ctx.press_any('Show the calls')
    ctx.press('AXButton', 'Close Explain')
    tree, close_button = ctx.app.wait_for(lambda t: ctx.app.node(t, 'AXButton', 'Close Explain') is None, 8)
    ctx.press_any('Show the calls')
    setup.switch(ctx, ctx.state['spare_tenant'])
    tree = ctx.app.tree()
    tree, tenant_closed = ctx.app.wait_for(lambda t: ctx.app.node(t, 'AXButton', 'Close Explain') is None, 8)
    setup.switch(ctx, ctx.state['bootstrap_tenant'])
    retired = ctx.app.node(ctx.app.tree(), 'AXButton', 'Close Explain') is None
    evidence = ctx.save('explain-native-observations', {'before_main': before, 'after_main': after, 'final_tree': ctx.app.tree()})
    ctx.record('C47-N02', {'overlay_did_not_change_main_width': same_width, 'page_refresh_remained_usable': control_live,
                          'navigation_remained_usable': navigation_live, 'page_escape_did_not_close_drawer': page_escape_kept,
                          'modal_escape_preserved_drawer': modal_priority, 'drawer_escape_closed': closed,
                          'focus_returned_to_explain': focus_back, 'close_button_worked': close_button,
                          'tenant_change_closed_and_retired_drawer': tenant_closed and retired}, [evidence], [drawer_capture])
    theme_images = []
    no_ask, sidebar, next_action = [], [], []
    for appearance, expected in (('Dark', 'light'), ('Light', 'dark')):
        c29.appearance(ctx.app, appearance)
        tree = ctx.navigate('Home')
        no_ask.append(not any(n.get('role') == 'AXButton' and n.get('label') == 'Ask Harness' for n in tree))
        sidebar.append(ctx.app.node(tree, 'AXButton', 'Harness') is not None)
        switch = ctx.app.node(tree, 'AXCheckBox', 'Switch light or dark appearance')
        ctx.require(switch, 'appearance switch absent')
        if switch:
            ctx.app.ax('focus', 'AXCheckBox', 'Switch light or dark appearance')
        theme_images.append(ctx.capture('shell-sun-'+appearance.lower()))
        ctx.press_any('Switch light or dark appearance')
        ctx.wait_state(lambda value: value['appearance'] == expected, 'appearance next action did not persist')
        next_action.append(True)
    ctx.record('C47-N03', {'redundant_ask_harness_absent': all(no_ask), 'sidebar_harness_retained': all(sidebar), 'labelled_next_action_worked': all(next_action),
                          'appearance_control_present': all(ctx.app.node(ctx.app.tree(), role, 'Switch light or dark appearance') is not None for role in ('AXCheckBox',))},
               [ctx.save('shell-sun-labels', {'next_action_labels': next_action, 'tree': ctx.app.tree()})], theme_images)
    c29.appearance(ctx.app, 'Match System')
    ctx.navigate('Home')

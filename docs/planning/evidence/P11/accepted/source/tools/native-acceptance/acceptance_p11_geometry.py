"""Exact shell viewport sizing, independent of a platform title-bar constant."""
import math
import time
import acceptance_p11_shell as shell


def _measure(ctx):
    frame = ctx.frame()
    areas = [n for n in ctx.app.tree() if n.get('role') == 'AXWebArea']
    ctx.require(len(areas) == 1, 'shell geometry requires one owned WebArea')
    viewport = areas[0].get('size')
    ctx.require(isinstance(viewport, list) and len(viewport) == 2 and
                all(type(n) in (int, float) and math.isfinite(n) and n > 0 for n in viewport),
                'shell viewport geometry is invalid')
    chrome = [frame[2] - viewport[0], frame[3] - viewport[1]]
    ctx.require(all(math.isfinite(n) and 0 <= n <= 200 for n in chrome),
                'shell native chrome geometry is invalid')
    return frame, viewport, chrome


def size_window(ctx, width, height):
    ctx.require(all(type(n) in (int, float) and math.isfinite(n) and n > 0 for n in (width, height)),
                'invalid required shell viewport')
    for attempt in range(1, 4):
        _frame, _viewport, chrome = _measure(ctx)
        try:
            # Leave placement alone: macOS can clamp y=0 to its menu bar,
            # and combining that move with a small resize lost the size change.
            ctx.resize(width + chrome[0], height + chrome[1])
        except RuntimeError as error:
            if not str(error).startswith('owned window resize did not reach requested geometry:'):
                raise
        time.sleep(.8)  # Same bounded WebKit settle as the original shell helper.
        frame, viewport, actual_chrome = _measure(ctx)
        ctx.log.append({'action': 'shell-viewport-measurement', 'attempt': attempt,
                        'required_viewport': [width, height], 'actual_viewport': viewport,
                        'actual_chrome': actual_chrome, 'window_frame': frame})
        if viewport == [width, height]:
            return frame
    raise RuntimeError(f'required shell viewport was not reached: {[width, height]} != {viewport}')


def run_shell(ctx):
    original = shell.size_window
    shell.size_window = size_window
    try:
        return shell.run(ctx)
    finally:
        shell.size_window = original

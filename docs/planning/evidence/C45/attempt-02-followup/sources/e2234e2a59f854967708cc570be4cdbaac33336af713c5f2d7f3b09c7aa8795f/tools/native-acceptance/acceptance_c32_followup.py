#!/usr/bin/env python3
"""Execute only C32's ended-status settling and enabled-composer Tab assertions.

Earlier completed native consent/proposal/layout/motion observations remain
immutable. This driver does not repeat them or claim their execution here.
"""
import json
import os
from pathlib import Path
import sys
import time
sys.path.insert(0, str(Path(__file__).resolve().parent))
import acceptance_c32_current as c32
import fixture_app as fx
import acceptance_c19 as c19
import acceptance_c29 as c29
import acceptance_c31 as c31
import native_sheets as sheets


def exercise(app, endpoint, mgmt, prepared, project, final):
    c32.setup(app, endpoint, mgmt)
    app.menu('Go', 'Harness')
    tree, ready = app.wait_for(lambda t: fx.App.node(t, 'AXTextArea', 'Message') is not None, 20)
    if not ready: raise RuntimeError('Harness composer unavailable')
    c32.plain(mgmt)
    c32.send(app, 'C32 focused conversation status check')
    _, answered = app.wait_for(lambda t: c32.answers(app,t)>=1 and fx.App.node(t,'AXButton','End conversation') is not None, 60)
    if not answered: raise RuntimeError('Synthetic conversation did not answer')
    app.type_into('AXTextArea', 'Message', 'Unsent C32 keyboard check')
    app.ax('focus', 'AXTextArea', 'Message')
    focused = fx.App.focused(app.tree()) or {}
    before = {'role':focused.get('role'), 'label':focused.get('label')}
    app.keycode(48)
    time.sleep(.3)
    tree=app.tree();focused=fx.App.focused(tree) or {}
    keyboard={'draft_entered':True,'before':before,'after':{'role':focused.get('role'),'label':focused.get('label')},'send_reached':focused.get('role')=='AXButton' and focused.get('label')=='Send','tree':c29.save_tree('c32-composer-tab-enabled',tree),'capture':c29.capture(app,'c32-composer-tab-enabled')}
    app.type_into('AXTextArea','Message','')
    c29.press(app,'AXButton','End conversation')
    gone=c19.wait(lambda:not c19.engines(),20)
    tree,announced=app.wait_for(lambda t:any('ended' in text.lower() for text in c31.status_texts(t)),20)
    regions=c31.status_texts(tree)
    status={'engine_gone':gone,'ended_announced':announced,'status_regions':regions,'saved_transcript':app.has_text(tree,'Transcript saved on this Mac'),'rail_ended':app.has_text(tree,'Ended'),'tree':c29.save_tree('c32-ended-status-settled',tree),'capture':c29.capture(app,'c32-ended-status-settled')}
    facts={'subject':'C32','app':prepared,'keyboard':keyboard,'ended_status':status,'limitations':['Only these two follow-up native assertions executed here; earlier completed facts are bound separately, never relabeled. Spoken VoiceOver output not recorded.']}
    path=Path(c32.NATIVE)/'followup-observations.json';path.write_text(json.dumps(facts,indent=2)+'\n')
    app.quit()
    return 0 if keyboard['send_reached'] and gone and announced and status['saved_transcript'] and status['rail_ended'] else 1


if __name__=='__main__':
    try: raise SystemExit(c32.run(exercise))
    finally:
        sheets.disable()
        fx.stop_app();fx.stop_engines();fx.clean_fixture_root()

"""P11 required-OAuth setup and warning flows. Called only by the owned driver."""
from pathlib import Path
import time
import acceptance_c08 as c8
import fixture_app as fx


def enabled(app, label):
    node = app.node(app.tree(), 'AXButton', label)
    return bool(node and node.get('enabled') is not False)


def draft(ctx, name):
    ctx.heading('Connect your tenant')
    ctx.app.type_into('AXTextField', 'Display name', name)
    ctx.app.type_into('AXTextField', 'TSG ID', c8.TSG)
    ctx.press('AXButton', 'Draft tenant')
    ctx.text(name+' is drafted. Add its management client below.')
    return not enabled(ctx.app, 'Save the management client to continue')


def client(ctx):
    ctx.app.type_into('AXTextField', 'Client ID', c8.CLIENT_ID)
    ctx.app.type_into('AXTextField', 'Client secret', c8.SECRET, secure=True)
    ctx.press('AXButton', 'Save management client')
    ctx.text('Management client saved.')
    ctx.require(ctx.app.node(ctx.app.tree(), 'AXButton', 'Replace management client'), 'management credential did not settle')


def verification(ctx, model=False):
    ctx.press('AXButton', 'Continue')
    ctx.heading('Which products will you manage?')
    if model:
        node = ctx.app.node(ctx.app.tree(), 'AXCheckBox', 'AI Supply Chain Security')
        ctx.require(node, 'Supply Chain product choice absent')
        if node.get('value') not in (True, 1, '1', 'true'):
            ctx.press('AXCheckBox', 'AI Supply Chain Security')
    ctx.press('AXButton', 'Continue')
    ctx.heading('Check the connection')
    ctx.press('AXButton', 'Run the checks')


def finish(ctx, name, first):
    tree, ready = ctx.app.wait_for(lambda t: enabled(ctx.app, 'Continue with warnings') or enabled(ctx.app, 'Continue'), 45)
    ctx.require(ready, 'verification never made completion available')
    ctx.press('AXButton', 'Continue with warnings' if ctx.app.node(tree, 'AXButton', 'Continue with warnings') else 'Continue')
    ctx.heading('Load starter test cases')
    ctx.press('AXButton', 'Skip for now')
    if first:
        ctx.heading('Join the AI Security Academy')
        ctx.press('AXButton', 'Skip for now')
    ctx.heading(name+' is ready')
    ctx.press('AXButton', 'Open Prisma AIRS' if first else 'Finish')
    return ctx.wait_state(lambda s: any(t['label'] == name for t in s['tenants']) and not s.get('tenantDraft'), 'tenant did not commit')


def switch(ctx, name):
    ctx.app.menu('Go', 'Switch Tenant…')
    ctx.press_any(name)
    return ctx.wait_state(lambda s: next((t['label'] for t in s['tenants'] if t['id'] == s.get('activeTenant')), None) == name, 'tenant switch did not settle')


def bootstrap(ctx):
    ctx.mgmt.mode(grant='deny', profiles='deny', skillStatsStatus=403, keyExpiresInDays=3)
    ctx.app.launch()
    ctx.heading('Welcome to Prisma AIRS Desktop')
    evidence = [ctx.save('oauth-welcome')]
    ctx.press('AXButton', 'Set up a tenant')
    facts = {'missing_oauth_cannot_continue': draft(ctx, ctx.state['bootstrap_tenant'])}
    evidence.append(ctx.save('oauth-required-before-client'))
    client(ctx)
    verification(ctx, model=True)
    ctx.text('The management client was not accepted.', timeout=45)
    facts['denied_oauth_cannot_continue'] = not enabled(ctx.app, 'Continue') and not enabled(ctx.app, 'Continue with warnings')
    facts['no_commit_after_oauth_failure'] = len(ctx.fixture_state()['tenants']) == 0
    evidence.append(ctx.save('oauth-denied'))
    ctx.mgmt.mode(grant='ok', profiles='deny', skillStatsStatus=403, keyExpiresInDays=3)
    ctx.press('AXButton', 'Verify again')
    ctx.text('Management OAuth authenticated successfully.', timeout=45)
    ctx.text('Skill statistics:', timeout=45)
    facts['oauth_success_distinct_from_product_denial'] = enabled(ctx.app, 'Continue with warnings')
    evidence.append(ctx.save('oauth-success-product-denied'))
    doc = finish(ctx, ctx.state['bootstrap_tenant'], True)
    ctx.heading('Home')
    tenant = next(t for t in doc['tenants'] if t['label'] == ctx.state['bootstrap_tenant'])
    ctx.state['bootstrap_tenant_id'] = tenant['id']
    original = next(w for row in doc['tenantWarnings'] if row['tenant'] == tenant['id'] for w in row['warnings'] if w['product'] == 'modelSkill')
    ctx.state['original_supply_chain_warning'] = original
    facts['scan_key_optional_without_validation_scan'] = not any(c['tenant'] == tenant['id'] and c.get('class', 'scan') == 'scan' for c in doc['credentials']) and len(ctx.scan.requests()) == 0
    facts['oauth_was_actual_token_request'] = any(r.get('grantType') == 'client_credentials' and r.get('status') == 200 for r in ctx.mgmt.requests())
    facts['oauth_verify_not_profile_probe'] = not any('/profiles/' in r.get('path', '') for r in ctx.mgmt.requests())
    # Add/Cancel saves only a disposable draft's secret and must keep prior active.
    ctx.navigate('Settings')
    accounts = fx.keychain_accounts(c8.SCAN_SERVICE)
    ctx.press('AXButton', 'Add tenant…')
    facts['add_also_requires_oauth'] = draft(ctx, 'P11 Cancelled')
    client(ctx)
    evidence.append(ctx.save('add-draft-before-cancel'))
    ctx.press('AXButton', 'Cancel')
    after = ctx.wait_state(lambda s: not s.get('tenantDraft') and not s.get('pendingCleanup'), 'cancelled draft cleanup did not settle')
    facts['cancel_preserved_prior_active_and_tenants'] = after['activeTenant'] == tenant['id'] and [t['id'] for t in after['tenants']] == [tenant['id']]
    deadline = time.monotonic()+12
    while time.monotonic() < deadline and fx.keychain_accounts(c8.SCAN_SERVICE) != accounts:
        time.sleep(.3)
    facts['cancel_removed_only_draft_fixture_credentials'] = fx.keychain_accounts(c8.SCAN_SERVICE) == accounts
    evidence.append(ctx.save('add-cancelled'))
    # A fully authenticated second owned profile supports real tenant-switch UI.
    ctx.press('AXButton', 'Add tenant…')
    draft(ctx, ctx.state['spare_tenant'])
    client(ctx)
    verification(ctx)
    ctx.text('Management OAuth authenticated successfully.', timeout=45)
    finish(ctx, ctx.state['spare_tenant'], False)
    switch(ctx, ctx.state['bootstrap_tenant'])
    ctx.navigate('Home')
    facts['add_committed_only_after_actual_oauth'] = len(ctx.fixture_state()['tenants']) == 2
    facts['no_validation_scan_in_any_setup'] = len(ctx.scan.requests()) == 0
    ctx.record('C46-N01', facts, evidence)


def warning_flows(ctx):
    switch(ctx, ctx.state['bootstrap_tenant'])
    ctx.mgmt.mode(grant='ok', profiles='deny', skillStatsStatus=403, keyExpiresInDays=3)
    ctx.navigate('Inbox')
    original = ctx.state['original_supply_chain_warning']
    title = 'AI Supply Chain Security: setup warning'
    ctx.press('AXButton', title)
    before = ctx.save('warning-original')
    ctx.text('Skill statistics:')
    ctx.press('AXButton', 'Refresh')
    ctx.text(title)
    retained_refresh = not warning_state(ctx, original['id']).get('resolvedSecs')
    ctx.press('AXButton', title)
    ctx.press('AXButton', 'Open AI Supply Chain Security')
    admin = ctx.heading('Administration')
    supply_arrival = ctx.app.node(admin, 'AXRadioButton', 'Model scans') or ctx.app.node(admin, 'AXButton', 'Model scans') or any('Model scans' in t for t in ctx.app.texts(admin))
    arrival = ctx.save('warning-supply-chain-arrival', admin)
    # The current fixture's expiring key supplies an ordinary Runtime item.
    ctx.navigate('Inbox')
    runtime = next((n for n in ctx.app.tree() if n.get('role') == 'AXButton' and ('expires in' in (n.get('label') or '').lower() or 'expired' in (n.get('label') or '').lower())), None)
    ctx.require(runtime, 'owned expiring Runtime key warning absent')
    ctx.press('AXButton', runtime['label'])
    ctx.press('AXButton', 'Open API keys')
    runtime_tree = ctx.heading('Administration')
    runtime_filter = ctx.app.node(runtime_tree, 'AXRadioButton', 'API keys') or any('API keys' in t for t in ctx.app.texts(runtime_tree))
    runtime_evidence = ctx.save('warning-runtime-resource-arrival', runtime_tree)
    ctx.navigate('Inbox')
    ctx.press('AXButton', title)
    ctx.press('AXButton', 'Recheck setup')
    ctx.text('Latest check: Warning', timeout=45)
    failed = ctx.save('warning-recheck-permission-denied')
    failed_active = warning_state(ctx, original['id']).get('resolvedSecs') is None
    ctx.mgmt.mode(grant='ok', profiles='deny', skillStatsStatus=200, keyExpiresInDays=3)
    ctx.press('AXButton', 'Recheck setup')
    ctx.wait_state(lambda s: bool(warning_state(ctx, original['id']).get('resolvedSecs')), 'matching setup check never resolved', timeout=45)
    resolved = warning_state(ctx, original['id'])
    ctx.navigate('Settings')
    history = ctx.text('Setup check history')
    ctx.text(original['reason'])
    history_path = ctx.save('warning-dated-history', history)
    image = ctx.capture('warning-history')
    ctx.record('C46-N02', {'ordinary_refresh_kept_original_finding': retained_refresh,
                          'precise_failed_source_visible': ctx.app.has_text(ctx.app.tree(), 'Skill statistics:'),
                          'failed_recheck_remained_active': failed_active,
                          'matching_fresh_pass_resolved': bool(resolved.get('resolvedSecs')) and resolved['recheck']['state'] == 'pass',
                          'original_reason_date_unchanged': resolved['reason'] == original['reason'] and resolved['recordedSecs'] == original['recordedSecs'],
                          'diagnostics_history_visible': ctx.app.has_text(ctx.app.tree(), 'Setup check history')},
               [before, failed, history_path], [image])
    ctx.record('C46-N03', {'supply_chain_product_arrival': bool(supply_arrival),
                          'runtime_resource_arrival_retained': bool(runtime_filter),
                          'dated_history_and_latest_provenance_visible': ctx.app.has_text(ctx.app.tree(), 'Latest check: Passed') and ctx.app.has_text(ctx.app.tree(), 'Originally recorded'),
                          'no_product_write_or_scan': len(ctx.scan.requests()) == 0 and not any(r.get('method') in ('PUT', 'PATCH', 'DELETE') for r in ctx.mgmt.requests())},
               [arrival, runtime_evidence, history_path], [image])


def warning_state(ctx, warning_id):
    doc = ctx.fixture_state()
    row = next(row for row in doc['tenantWarnings'] if row['tenant'] == ctx.state['bootstrap_tenant_id'])
    return next(w for w in row['warnings'] if w['id'] == warning_id)

"""P11 per-assertion native origins. Never synthesizes a passing process run."""
from pathlib import Path
import argparse
import importlib.util
import json
import platform
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'p10'))
from qualification import bound, read, require, sha
from candidate import helper_closure


def reference(root, path):
    path = Path(path).resolve()
    require(path.is_relative_to(root.resolve()), 'native evidence escapes checkout')
    return {'path': str(path.relative_to(root)), 'sha256': sha(path)}


def inventory_root(root):
    archived = root/'docs/planning/evidence/P11/source-history/S2'
    return archived if archived.is_dir() else root


def inventory(root):
    chunks = read(inventory_root(root)/'docs/architecture/native-checks.json')['chunks']
    return {row['id']: row['check'] for chunk in ('C46', 'C47', 'C48', 'C49') for row in chunks[chunk]}


def c46_helpers(root):
    archived = root/'docs/planning/evidence/P11/source-history/S2'
    paths = set(helper_closure(archived if archived.is_dir() else root, 'acceptance_p11_setup'))
    paths.add('tools/native-acceptance/acceptance_p11.py')
    require('tools/native-acceptance/acceptance_p11_api.py' not in paths,
            'C46 scope unexpectedly depends on changed API helper')
    return sorted(paths)



def positive_rows(raw):
    return {row['id']: row for row in raw['checks']
            if row['facts'] and all(value is True for value in row['facts'].values())}


def excluded_checks(raw):
    return [{'id': row['id'],
             'failed_facts': sorted(name for name, value in row['facts'].items() if value is not True),
             'reason': 'Measured assertion was not completed; a later positive replacement is required.'}
            for row in raw['checks'] if row['id'] not in positive_rows(raw)]


def artifact_carry(root, raw, candidate, prior_refs):
    """Only the diagnosed N01 layout facts/captures may support its repair."""
    carry = raw.get('artifact_carry')
    if not carry: return {}
    require(carry['assertion'] == 'C47-N01' and carry['observations'] in prior_refs,
            'unapproved native artifact carry')
    path = bound(root, carry['observations'])
    old = read(path)
    rows = [row for row in old['checks'] if row['id'] == 'C47-N01']
    require(len(rows) == 1 and rows[0]['facts'].get('named_actions_keyboard_reachable') is False
            and all(value is True for name, value in rows[0]['facts'].items()
                    if name != 'named_actions_keyboard_reachable'),
            'N01 carry does not retain the original isolated keyboard failure')
    require(old['executable_sha256'] == candidate['executable_sha256']
            and old['bundle_sha256'] == candidate['bundle_sha256']
            and old['binding']['conditions'] == candidate['conditions']
            and all(candidate['helper_closure'].get(name) == digest
                    for name, digest in old['helper_closure'].items()),
            'N01 retained layout artifact/helper/VM conditions differ')
    allowed = set(rows[0]['evidence']+rows[0]['visual'])
    repaired = [row for row in raw['checks'] if row['id'] == 'C47-N01']
    matrices = [name for name in rows[0]['evidence'] if Path(name).name == 'shell-layout-measurements.json']
    require(len(repaired) == 1 and len(matrices) == 1
            and matrices[0] in repaired[0]['evidence']
            and set(rows[0]['visual']) <= set(repaired[0]['visual'])
            and {matrices[0], *rows[0]['visual']} <= set(carry['artifact_sha256']),
            'N01 repair omitted retained matrix or capture review scope')
    require(carry['artifact_sha256'] and set(carry['artifact_sha256']) <= allowed,
            'N01 carry includes unrelated artifacts')
    for name, digest in carry['artifact_sha256'].items():
        require((root/name).resolve().is_relative_to(path.parent)
                and old['artifact_sha256'].get(name) == digest == sha(root/name),
                'retained N01 layout artifact changed')
    return carry['artifact_sha256']


def origins(root, selection, candidate):
    require(selection['schema'] == 'airs-desktop/p11-native-selection/1'
            and selection['native_inventory_sha256'] == sha(inventory_root(root)/'docs/architecture/native-checks.json'),
            'native selection scope differs')
    prepare = read(bound(root, selection['prepare']))
    require(prepare['executable_sha256'] == candidate['executable_sha256']
            and prepare['bundle_sha256'] == candidate['bundle_sha256'], 'native candidate differs')
    context = read(bound(root, selection['source_context']))
    require(all(context[key] == candidate[key] for key in
                ('executable_sha256', 'bundle_sha256', 'build_flags', 'conditions')),
            'completed native assertion conditions changed')
    require(len(selection['origins']) >= 2, 'native continuation needs its actual contributing runs')
    selected, prior_refs = {}, []
    for i, origin in enumerate(selection['origins']):
        path = bound(root, origin['observations'])
        raw = read(path)
        final = i == len(selection['origins'])-1
        all_rows = {row['id']: row for row in raw['checks']}
        rows = positive_rows(raw)
        ids = sorted(rows)
        require(len(all_rows) == len(raw['checks']) and origin['ids'] == ids
                and origin.get('excluded_checks', []) == excluded_checks(raw)
                and set(ids) <= set(inventory(root))-set(selected)
                and origin['disposition'] == ('executed' if final else 'reused_exact'),
                'native origin membership or disposition differs')
        if i == 0: require(ids == ['C46-N01'], 'initial completed setup scope differs')
        driver = Path(raw['argv'][1]).name
        require(raw['schema'] == 'airs-desktop/p11-native-observations/1' and raw['subject'] == 'P11'
                and raw['execution_id'] == path.parent.name and raw['performed_by']
                and len(raw['argv']) >= 2 and Path(raw['argv'][0]).name.lower() in ('python3', 'python')
                and raw['argv'][1] in [
                    'tools/native-acceptance/acceptance_p11.py',
                    'tools/native-acceptance/acceptance_p11_resume.py',
                    'tools/native-acceptance/acceptance_p11_continue.py',
                    'tools/native-acceptance/acceptance_p11_finish.py', 'tools/native-acceptance/acceptance_p11_last.py',
                    'tools/native-acceptance/acceptance_p11_verify.py', 'tools/native-acceptance/acceptance_p11_warn.py']
                and (i != 0 or driver == 'acceptance_p11.py'),
                'native origin execution identity differs')
        require(raw['executable_sha256'] == candidate['executable_sha256']
                and raw['bundle_sha256'] == candidate['bundle_sha256']
                and raw['host'] == candidate['conditions']['platform'],
                'native origin artifact/platform changed')
        require(raw['binding']['prepare']['sha256'] == selection['prepare']['sha256']
                and raw['binding']['native_inventory_sha256'] == selection['native_inventory_sha256']
                and raw['binding']['helper_closure'] == raw['helper_closure'],
                'native origin aliases differ')
        require(time.strptime(raw['finished_utc'], '%Y-%m-%dT%H:%M:%SZ')
                >= time.strptime(raw['started_utc'], '%Y-%m-%dT%H:%M:%SZ'), 'native origin time invalid')
        if not final:
            require(raw['complete'] is False and raw['status'] == 'failed' and raw['exit_code'] == 1
                    and raw['errors'], 'original failed process provenance was relabelled')
        else:
            require(raw['complete'] is True and raw['status'] == 'observed' and raw['exit_code'] == 0
                    and not raw['errors'] and set(ids) == set(inventory(root))-set(selected),
                    'remaining native execution did not complete')
        if i == 0:
            require(raw['helper_closure'] == context['helper_closure'],
                    'completed run differs from bound S2 helper context')
            paths = c46_helpers(root)
        else:
            admission = raw['continuation']
            admitted_origins = admission.get('origins', [admission['observations']])
            require(raw['binding']['conditions'] == candidate['conditions']
                    and admitted_origins == prior_refs
                    and admission['observations'] == prior_refs[-1]
                    and sorted(admission['completed_ids']) == sorted(selected)
                    and sorted(admission['remaining_ids']) == sorted(set(inventory(root))-set(selected))
                    and admission['fixture_state_semantically_equal'] is True
                    and admission['certificate_generation_or_install_executed'] is False,
                    'native continuation admission/provenance differs')
            admission_path = path.parent/'resume-preconditions.json'
            require(raw['artifact_sha256'].get(str(admission_path.relative_to(root))) == sha(admission_path)
                    and read(admission_path) == admission, 'recorded admission proof differs')
            # All 07 driver files remain unchanged; no scope is silently dropped.
            paths = sorted(raw['helper_closure'])
        require(origin['helper_paths'] == paths and all(
            raw['helper_closure'].get(name) == candidate['helper_closure'].get(name)
            for name in paths), 'relevant native helper changed')
        if final:
            require(raw['helper_closure'] == candidate['helper_closure'], 'current native driver closure differs')
        for artifact, digest in raw['artifact_sha256'].items():
            file = (root/artifact).resolve()
            require(file.is_relative_to(path.parent) and sha(file) == digest, 'native origin artifact changed')
        combined_artifacts = dict(raw['artifact_sha256'])
        combined_artifacts.update(artifact_carry(root, raw, candidate, prior_refs))
        for check_id, row in rows.items():
            require(row['facts'] and all(value is True for value in row['facts'].values()),
                    'native assertion has incomplete or failed facts')
            for name in row['evidence']+row['visual']:
                require(name in combined_artifacts and sha(root/name) == combined_artifacts[name],
                        'native row evidence was not captured by its contributing run')
            selected[check_id] = dict(raw_path=path, raw=raw, row=row,
                                     helper_paths=paths, disposition=origin['disposition'])
        prior_refs.append(origin['observations'])
    return selected


def review_captures(root, selection, candidate, selected):
    review = read(bound(root, selection['visual_review']))
    require(review['schema'] == 'airs-desktop/p11-visual-review/2'
            and review['observations'] == [origin['observations'] for origin in selection['origins']]
            and review['executable_sha256'] == candidate['executable_sha256']
            and review['bundle_sha256'] == candidate['bundle_sha256'] and review['performed_by'],
            'capture review does not bind both actual origins/candidate')
    wanted = {(image, cid) for cid, item in selected.items() for image in item['row']['visual']}
    images = {(row['path'], row['assertion']): row for row in review['captures']}
    require(len(images) == len(review['captures']) and set(images) == wanted,
            'capture-specific review membership differs')
    for (name, _), row in images.items():
        require(row['sha256'] == sha(root/name) and row['accepted'] is True and not row['findings'],
                'capture review failed or image changed')

def validate(root, acceptance_path, execution_path, candidate):
    """Per-row process outcome remains distinct from the completed assertion."""
    selection = read(execution_path)
    selected = origins(root, selection, candidate)
    review_captures(root, selection, candidate, selected)
    acceptance = read(acceptance_path)
    require(acceptance['schema'] == 'airs-v2/native-acceptance/1' and acceptance['subject'] == 'P11'
            and acceptance['method'] == 'cua' and acceptance['app']['fixture'] is True
            and acceptance['app']['executable_sha256'] == candidate['executable_sha256'],
            'final native acceptance candidate differs')
    checks = {row['id']: row for row in acceptance['checks']}
    require(len(checks) == len(acceptance['checks']) and set(checks) == set(selected),
            'final native check membership differs')
    for cid, item in selected.items():
        row = checks[cid]
        evidence = set(item['row']['evidence']+item['row']['visual']+
            [str(item['raw_path'].relative_to(root)), str(execution_path.relative_to(root)),
             selection['visual_review']['path']])
        require(row['result'] == 'pass' and row['observation'] and evidence <= set(row['evidence']),
                'final native row lost its actual origin/evidence')
    return selected


def finalize(args):
    root = Path.cwd().resolve()
    previous, continuation = [path.resolve() for path in args.previous], args.continuation.resolve()
    last = read(continuation)
    prepare = Path(last['binding']['prepare']['path']).resolve()
    prepared = read(prepare)
    candidate = dict(executable_sha256=prepared['executable_sha256'],
        bundle_sha256=prepared['bundle_sha256'],
        helper_closure=helper_closure(root, Path(last['argv'][1]).stem),
        build_flags={'target': prepared['target'], 'features': prepared['features']},
        conditions=last['binding']['conditions'])
    selection = dict(schema='airs-desktop/p11-native-selection/1',
        prepare=reference(root, prepare), source_context=reference(root, args.source_context),
        native_inventory_sha256=sha(inventory_root(root)/'docs/architecture/native-checks.json'),
        visual_review=reference(root, args.visual_review), origins=[
            dict(observations=reference(root, path), ids=sorted(positive_rows(read(path))), excluded_checks=excluded_checks(read(path)),
                 disposition='reused_exact' if i < len(previous) else 'executed',
                 helper_paths=c46_helpers(root) if i == 0 else sorted(read(path)['helper_closure']))
            for i, path in enumerate([*previous, continuation])])
    selected = origins(root, selection, candidate)
    review_captures(root, selection, candidate, selected)
    out = args.out.resolve()
    require(out.is_relative_to(root) and not out.exists(), 'preserve existing native finalization')
    out.mkdir(parents=True)
    selection_path, acceptance_path = out/'native-selection.json', out/'native-acceptance.json'
    selection_path.write_text(json.dumps(selection, indent=2)+'\n')
    checks = []
    for cid, description in inventory(root).items():
        item = selected[cid]
        checks.append(dict(id=cid, description=description, result='pass',
            observation='Completed measured assertion '+cid+' from '+item['raw']['execution_id']+
                '; contributing process exit '+str(item['raw']['exit_code'])+
                ' remains recorded. Capture-specific visual review is separately bound. '
                'Source boundaries require the separate qualification; no source suite is relabelled.',
            evidence=[str(selection_path.relative_to(root)), str(item['raw_path'].relative_to(root)),
                      selection['visual_review']['path'], *item['row']['evidence'], *item['row']['visual']]))
    acceptance_path.write_text(json.dumps(dict(schema='airs-v2/native-acceptance/1', subject='P11',
        recorded_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()), method='cua',
        performed_by=last['performed_by'], host=last['host'],
        app={'bundle': prepared['bundle'], 'identifier': prepared['identifier'],
             'executable_sha256': prepared['executable_sha256'], 'fixture': True},
        checks=checks, limitations=[
            'Synthetic VM fixture only; no stable/live or Linux acceptance.',
            'C46-N01 completed in an overall failed earlier process. Its failure is preserved.',
            'This is per-assertion evidence accounting, not a passing composite process.']),
        indent=2)+'\n')
    validate(root, acceptance_path, selection_path, candidate)
    return 0


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['finalize'])
    parser.add_argument('--previous', type=Path, nargs='+', required=True)
    for name in ('continuation', 'visual-review', 'source-context', 'out'):
        parser.add_argument('--'+name, type=Path, required=True)
    raise SystemExit(finalize(parser.parse_args()))

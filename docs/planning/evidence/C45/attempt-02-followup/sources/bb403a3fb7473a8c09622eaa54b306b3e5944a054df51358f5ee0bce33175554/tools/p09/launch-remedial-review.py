from pathlib import Path
import hashlib,json,subprocess,sys

root=Path.cwd().resolve();subject,attempt=sys.argv[1:3]
out=root/f'docs/planning/reviews/{subject}/{attempt}'
assert not any((out/n).exists() for n in ['verdict-returned.json','verdict.json','receipt.json','thread-reply.log','invocation.json','feature-probe.log']), 'preserve prior launches; choose a fresh attempt'
if len(sys.argv)>3:
    model=sys.argv[3].strip()
    if not model:
        raise SystemExit('The explicit review model is empty; pass a configured model as argument 3.')
else:
    config_path=Path.home()/'.codex/config.toml'
    try:
        import tomllib
    except ModuleNotFoundError as error:
        raise SystemExit('Reading the Codex config requires Python 3.11 or newer; pass the model as argument 3 instead.') from error
    try:
        config=tomllib.loads(config_path.read_text())
    except (OSError,tomllib.TOMLDecodeError) as error:
        raise SystemExit(f'No review model supplied and cannot read {config_path}: {error}. Pass the model as argument 3.') from error
    model=config.get('model')
    if not isinstance(model,str) or not model.strip():
        raise SystemExit(f'No top-level model is configured in {config_path}; pass the model as argument 3.')
    model=model.strip()
restricted=['hooks','plugins','apps','remote_plugin','browser_use','browser_use_external','in_app_browser',
            'computer_use','image_generation','multi_agent','skill_search','skill_mcp_dependency_install','unbounded_connection_retries']
feature_flags=[arg for name in restricted for arg in ['--disable',name]]
probe=subprocess.run(['codex','--no-daemon',*feature_flags,'features','list'],cwd=root,capture_output=True,text=True,check=True)
probe_text=probe.stdout+probe.stderr
(out/'feature-probe.log').write_text(probe_text)
observed={line.split()[0]:line.split()[-1] for line in probe.stdout.splitlines() if len(line.split())>=3}
assert all(observed.get(name)=='false' for name in restricted),'review feature isolation not observed'
argv=['codex','--no-daemon','-a','never',*feature_flags,'exec','-s','read-only','--skip-git-repo-check','--ephemeral','--ignore-user-config','--ignore-rules','-m',model,
      '--json','--color','never','-c','web_search="disabled"',
      '-c','model_reasoning_effort="xhigh"','--output-schema','docs/architecture/review-verdict.schema.json',
      '-o',str(out/'verdict-returned.json'),(out/'message.txt').read_text()]
(out/'invocation.json').write_text(json.dumps({'command_without_prompt':argv[:-1],'model_requested':model,
    'harness_version_observed':subprocess.check_output(['codex','--version'],cwd=root,text=True,stderr=subprocess.STDOUT).strip(),
    'request_sha256':hashlib.sha256((out/'request.json').read_bytes()).hexdigest(),
    'schema_sha256':hashlib.sha256((root/'docs/architecture/review-verdict.schema.json').read_bytes()).hexdigest(),
    'feature_probe_sha256':hashlib.sha256((out/'feature-probe.log').read_bytes()).hexdigest(),
    'restricted_features_observed':{name:observed[name] for name in restricted},
    'limits':'CLI arguments and feature probe establish requested isolation; inspect actual run events and reviewer limitations. No provider/model authorship or review pass is inferred.'},indent=2)+'\n')
with (out/'thread-reply.log').open('w') as log:
    result=subprocess.run(argv,cwd=root,stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT)
    log.write('\nCODEX EXIT '+str(result.returncode)+'\n')
if (out/'verdict-returned.json').exists():
    verdict=json.loads((out/'verdict-returned.json').read_text())
    print(subject,attempt,verdict['scores'],verdict['recommendation'])
else:
    print('no returned verdict; CLI exit',result.returncode)

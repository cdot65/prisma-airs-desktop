from pathlib import Path
import json,subprocess,sys

root=Path.cwd().resolve();subject,attempt=sys.argv[1:3]
out=root/f'docs/planning/reviews/{subject}/{attempt}'
assert not any((out/n).exists() for n in ['verdict-returned.json','verdict.json','receipt.json'])
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
argv=['codex','--no-daemon','-a','never','exec','-s','read-only','--skip-git-repo-check','--ephemeral','--ignore-user-config','--ignore-rules','-m',model,
      '--json','--color','never','-c','web_search="disabled"',
      '-c','model_reasoning_effort="xhigh"','--output-schema','docs/architecture/review-verdict.schema.json',
      '-o',str(out/'verdict-returned.json'),(out/'message.txt').read_text()]
with (out/'thread-reply.log').open('w') as log:
    result=subprocess.run(argv,cwd=root,stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT)
    log.write('\nCODEX EXIT '+str(result.returncode)+'\n')
if (out/'verdict-returned.json').exists():
    verdict=json.loads((out/'verdict-returned.json').read_text())
    print(subject,attempt,verdict['scores'],verdict['recommendation'])
else:
    print('no returned verdict; CLI exit',result.returncode)

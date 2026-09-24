from pathlib import Path
import re,subprocess,time
project=Path(__file__).resolve().parents[1]
binary=str(project/'target/release/mono')
sections=['system','cpu','memory','storage','filesystems','temperature','network','services','boot','hardware','power','logs']
count=0
def run(args,code=0):
    global count
    start=time.monotonic()
    result=subprocess.run([binary,*args],capture_output=True,text=True,timeout=25)
    assert result.returncode==code,(args,result.stdout,result.stderr)
    count+=1
    return result.stdout,time.monotonic()-start
def validate(text,size):
    states=re.findall(r'^(System|CPU|Memory|Storage|Filesystems|Temperature|Network|Services|Boot|Hardware|Power|Logs)\s+(OK|WARNING|ERROR|UNKNOWN|SKIPPED)$',text,re.M)
    assert len(states)==size,(size,states,text)
    problems=sum(status in ['WARNING','ERROR'] for _,status in states)
    assert f'Problems found: {problems}' in text
    assert len(text.splitlines())<110
text,elapsed=run(['diagnose']);validate(text,12)
for section in sections:
    text,_=run(['diagnose',section]);validate(text,12 if section=='system' else 1)
run(['diagnose','invalid'],1);run(['diagnose','memory','extra'],1)
text,_=run(['diagnose','system','--dry-run']);assert 'nothing executed' in text and 'TCP/443' in text
run(['diagnose','bad','--dry-run'],1)
for args in [['help','diagnose'],['manual','diagnose'],['manual','backend','diagnose']]:run(args)
print(f'{count} diagnostics CLI checks passed; system report completed in {elapsed:.2f}s.')

from pathlib import Path
import subprocess,tempfile,os,json
project=Path(__file__).resolve().parents[1]
binary=str(project/'target/release/mono')
results=[]
def run(args, expected=0, cwd=None):
    r=subprocess.run([binary,*args],cwd=cwd,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,timeout=20)
    ok=(r.returncode==expected) if isinstance(expected,int) else r.returncode in expected
    results.append({'args':args,'code':r.returncode,'ok':ok,'output':(r.stdout+r.stderr)[:1600]})
    if not ok: raise RuntimeError(str(results[-1]))
    return r.stdout
catalog=(project/'COMMANDS.md').read_text()
names=[line.split('`')[1].split()[0] for line in catalog.splitlines() if line.startswith('## `')]
for name in names:
    run(['help',name]);run(['manual',name])
with tempfile.TemporaryDirectory(prefix='monux-test-') as tmp:
    run(['create','file','notes.txt'],cwd=tmp)
    Path(tmp,'notes.txt').write_text('Monux integration test\n')
    run(['create','file','notes.txt'],1,cwd=tmp)
    assert Path(tmp,'notes.txt').read_text()=='Monux integration test\n'
    run(['create','dir','backup'],cwd=tmp)
    run(['copy','notes.txt','backup/'],cwd=tmp)
    run(['move','backup/notes.txt','backup/moved.txt'],cwd=tmp)
    assert Path(tmp,'backup/moved.txt').read_text()=='Monux integration test\n'
    run(['find','*.txt',tmp]);run(['read',tmp+'/notes.txt'])
    run(['info',tmp+'/notes.txt']);run(['permissions',tmp+'/notes.txt']);run(['owner',tmp+'/notes.txt'])
    run(['--dry-run','delete',tmp+'/notes.txt'])
    run(['delete',tmp+'/notes.txt'],1)
    assert Path(tmp,'notes.txt').exists()
    for args in [['permissions',tmp+'/notes.txt','executable'],['owner',tmp+'/notes.txt','root'],['copy',tmp+'/notes.txt',tmp+'/copy'],['move',tmp+'/notes.txt',tmp+'/moved'],['edit',tmp+'/notes.txt'],['open',tmp]]:
        run(['--dry-run',*args])
for args in [['version'],['status'],['status','network'],['network'],['network','interfaces'],['ip'],['dns'],['ports'],['ports','22'],['disks'],['list','apps'],['list','services'],['usage'],['usage','/'],['logs','boot'],['check','network']]:
    run(args)
for args in [['install','firefox'],['uninstall','firefox'],['update'],['update','firefox'],['clean'],['create','user','alex'],['kill','4821'],['enable','sshd'],['disable','sshd'],['reboot'],['shutdown'],['sleep'],['connect','wifi','Home'],['disconnect','wifi'],['scan','wifi'],['scan','bluetooth'],['connect','bluetooth','11:22:33:44:55:66'],['disconnect','bluetooth','11:22:33:44:55:66'],['ip','eth0','add','192.0.2.2/24'],['dns','eth0','1.1.1.1'],['network','check','example.com'],['search','video editor'],['start','firefox'],['stop','firefox'],['ping','1.1.1.1']]:
    run(['--dry-run',*args])
for args in [['delete','/'],['delete','/etc'],['kill','0'],['kill','1'],['permissions','/tmp/file','999'],['--dry-run','network','check','example.com','0'],['install','--help'],['network','check','x','0'],['reboot'],['shutdown'],['sleep']]:
    run(args,1)
print(f'{len(results)} CLI checks passed; {len(names)} commands have help and manual.')

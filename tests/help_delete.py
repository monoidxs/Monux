from pathlib import Path
import os,pty,select,subprocess,tempfile,time,json
project=Path(__file__).resolve().parents[1]
binary=str(project/'target/release/mono')
count=0
def run(args,code=0,cwd=None,env=None):
    global count
    r=subprocess.run([binary,*args],cwd=cwd,env=env,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,timeout=10)
    assert r.returncode==code,(args,r.returncode,r.stdout,r.stderr)
    count+=1
    return r.stdout+r.stderr
def interactive(args,cwd,env,answer='yes',code=0):
    global count
    master,slave=pty.openpty()
    p=subprocess.Popen([binary,*args],cwd=cwd,env=env,stdin=slave,stdout=slave,stderr=slave)
    os.close(slave)
    result=b'';sent=False;deadline=time.monotonic()+10
    try:
        while time.monotonic()<deadline:
            if select.select([master],[],[],0.1)[0]:
                try: chunk=os.read(master,65536)
                except OSError: break
                if not chunk: break
                result+=chunk
                if b'Confirm by typing' in result and not sent:
                    os.write(master,(answer+'\n').encode());sent=True
            if p.poll() is not None: break
        p.wait(timeout=2)
        assert p.returncode==code,(args,p.returncode,result.decode())
        count+=1
        return result.decode()
    finally:
        if p.poll() is None:p.kill();p.wait()
        os.close(master)
with tempfile.TemporaryDirectory(dir=project / "target",prefix='mono-delete-') as d:
    root=Path(d);env=dict(os.environ,XDG_DATA_HOME=str(root/'data'))
    for name in ['help','manual','status','delete','network','update','create']:
        for verb in ['help','manual']:run([verb,name])
    text=run(['help','delete'])
    for heading in ['DELETE','Usage:','Examples:','Options:','More:','--force','--permanent','--dry-run']:assert heading in text
    for utility in ['pacman','systemctl','gio','rm -','man rm']:assert utility not in run(['manual','delete'])
    assert 'gio trash' in run(['manual','backend','delete'])
    run(['manual','definitely-unknown'],1)
    file=root/'notes.txt';file.write_text('keep me')
    for args in [['delete',str(file),'--dry-run'],['--dry-run','delete',str(file)],['delete','--dry-run',str(file)]]:
        result=run(args);assert 'корзину' in result and '/usr/bin/' not in result;assert file.exists()
    run(['delete',str(file),'--unknown'],1)
    run(['delete',str(file)],1)
    interactive(['delete',str(file)],d,env,answer='no',code=1);assert file.exists()
    interactive(['delete',str(file)],d,env)
    assert not file.exists()
    trash=root/'data/Trash'
    entries=list((trash/'files').iterdir());assert any(x.read_text()=='keep me' for x in entries)
    assert list((trash/'info').glob('*.trashinfo'))
    directory=root/'build';directory.mkdir();(directory/'artifact').write_text('build')
    run(['delete',str(directory),'--dry-run'],1)
    run(['delete',str(directory),'--force','--dry-run'])
    interactive(['delete',str(directory),'--force'],d,env);assert not directory.exists()
    file.write_text('permanent')
    n=len(list((trash/'files').iterdir()))
    interactive(['delete',str(file),'--permanent'],d,env)
    assert not file.exists() and len(list((trash/'files').iterdir()))==n
    directory.mkdir();(directory/'artifact').write_text('build')
    interactive(['delete',str(directory),'--force','--permanent'],d,env);assert not directory.exists()
    directory.mkdir()
    interactive(['delete',str(directory),'--permanent'],d,env);assert not directory.exists()
    file.write_text('symlink target')
    link=root/'link';link.symlink_to(file)
    interactive(['delete',str(link),'--permanent'],d,env)
    assert not link.is_symlink() and file.read_text()=='symlink target'
    for path in ['/etc','/usr','/','/proc/1']:
        run(['delete',path,'--force','--permanent','--dry-run'],1)
    # A broken trash location must never trigger a fallback to permanent removal.
    bad=root/'not-a-directory';bad.write_text('blocked')
    badenv=dict(env,XDG_DATA_HOME=str(bad))
    interactive(['delete',str(file)],d,badenv,code=1)
    assert file.read_text()=='symlink target'
print(f'{count} help/delete integration checks passed (real trash, permanent, force, cancellation, unavailable trash, symlink).')

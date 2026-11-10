import os,re,sys,json,mmap
root=sys.argv[1]; store=os.path.join(root,'nix/store')
names=sorted(os.listdir(store)); hashes={n[:32]:n for n in names}
pat=re.compile(rb'[0-9a-df-np-sv-z]{32}-')
refs={}; size={}
for n in names:
    r=set(); s=0; p=os.path.join(store,n)
    files=[p] if not os.path.isdir(p) or os.path.islink(p) else None
    def scan(f):
        global s
        if os.path.islink(f):
            t=os.readlink(f).encode()
            for m in pat.findall(t): r.add(m[:32].decode())
            return
        if not os.path.isfile(f): return
        sz=os.path.getsize(f); s+=sz
        if sz==0: return
        try:
            with open(f,'rb') as fh:
                data=fh.read()
        except OSError: return
        for m in pat.findall(data): r.add(m[:32].decode())
    if files: scan(p)
    else:
        for d,ds,fs in os.walk(p):
            for x in ds:
                if os.path.islink(os.path.join(d,x)): scan(os.path.join(d,x))
            for x in fs: scan(os.path.join(d,x))
    refs[n]=sorted(hashes[h] for h in r if h in hashes and hashes[h]!=n); size[n]=s
json.dump({'refs':refs,'size':size},open(sys.argv[2],'w'))

# GitHub Publish

## 1) Set remote

```bash
./scripts/setup-github-remote.sh git@github.com:avikalpa/yggdrasil.git
```

## 2) Push `main`

```bash
git push -u origin main
```

## 3) Verify

```bash
git remote -v
git ls-remote --heads origin
```

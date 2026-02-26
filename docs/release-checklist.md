# Release Checklist

1. Run `./mkconfig.sh --profile both`
2. Confirm both ISO artifacts are produced
3. Run `./tests/smoke/run.sh --profile both --require-artifacts`
4. Boot-test both artifacts and validate runtime services
5. Publish only after both profiles pass

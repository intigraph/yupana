# Landing admission hook demonstration

The recording runs real pre-bash hook processes against an isolated block-tier
policy. A raw merge without admission returns a denial; the same PR with a
current issuer-signed admission returns no denial. Keys, catalogue and spool
belong to temporary test fixtures. Runtime publication and the advice soak
remain separate acceptance work.

Run the scenario with:

```sh
just test --features quipu --test landing_admission \
  controlled_block_refuses_raw_and_admits_current_helper_evidence -- --nocapture
```

Replay the recorded terminal output with:

```sh
scriptreplay --timing timing.txt --log-out output.txt
```

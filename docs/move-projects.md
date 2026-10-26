# Move the projects to Layr Cloud

Status: preparation and disposable rehearsal. Do not run the real-project steps yet.
The lead coordinates igor's steps after reviewing the rehearsal.

Each step names its actor. An **igor** step runs in igor's own Terminal or browser.
An agent must not run it, capture its terminal output, inspect its process environment, or read its recovery material.
An **agent** step uses public metadata, its own enrolled machine identity, and grants from SSM or the environment.

## Gates before a real move

- **igor / agent — use Layr main `ec8ba0f` or newer.** Task `work-1793009127566503168-7` is reviewed and merged. This revision includes the complete Git DAG transfer, identical commit-ID export and the mirror non-fast-forward guard. It also includes terminal-only paper-key creation from `879a228`. The old `c1d7229` network baseline does not preserve full history.
- **igor — use terminal-only store creation.** Reviewed Layr `879a228` adds `--paper-key-tty`. It sends the new paper key only to your controlling terminal and requires final-word confirmation before publication. The synthetic PTY rehearsal passed with no words in files. Do not use `--paper-key-out` or point it at `/dev/tty` for a real project. The final migration CLI is main `ec8ba0f` or newer, which includes both changes.
- **igor — accept invites in the signed-in site session.** Site-accept is merged as `fb6b424` and deployed on `cloud.layr.ws`. Use **You > Your invites > Accept**. It updates the encrypted index in your existing session; a real person-key file is not part of this procedure.
- **agent — wait for the reviewed gateway grant command and IAM allow list.** The new source accepts project-bound owner/executor/runner parameter names; the currently deployed gateway accepts the two legacy service names only. Before step 11, deploy the reviewed gateway release and apply the new exact-name PutParameter IAM plan from `project_iam.py`. This work is prepared but was not installed during the rehearsal.
- **agent — wait for multi-project runtime approval before installation.** Keep the existing single-project executor profile during the rehearsal. The prepared runtime uses one public profile and one SSM grant per project, and continues after a project's failure. Install it only after igor finishes the corresponding enrollment steps.
- **igor / agent — use the reviewed laptop runner.** Read the core repository's `docs/laptop-runner.md` and `tools/checks/configure.sh`. Trust its public machine identities. No paper key goes to a Linux runner.

Record the exact `ec8ba0f` or newer CLI revision and binary hash before the move. The old `c1d7229` binary is only the network-rehearsal baseline.

## Project stores

**agent — prepare one stack per project.** Use the unchanged `infra/aws/template.yaml`.
Each project has its own bucket and reader, writer and GC roles. Do not combine their descriptors or widen object-path grants.

| Project | Stack | Prefix | Bucket |
| --- | --- | --- | --- |
| `layr` | `layr-store-layr` | `layr` | `layr-store-layr-bucket-mg1n1j6rlomt` |
| `layr-cloud` | `layr-store-layr-cloud` | `layr-cloud` | `layr-store-layr-cloud-bucket-rqsdhssfnnca` |
| `layr-studio` | `layr-store-layr-studio` | `layr-studio` | `layr-store-layr-studio-bucket-8xbg6olreskn` |

The two added stores use SSE-S3 AES256, not customer-managed KMS keys. Their empty buckets and IAM roles add $0 fixed monthly cost. S3 storage and requests remain metered. See [AWS S3 billing](https://docs.aws.amazon.com/AmazonS3/latest/userguide/GetStartedWithS3.html).

**agent — update IAM through inspected CloudFormation change sets.** Preserve existing parameters. The gateway assumes exact reader/writer/GC ARNs for each project. The executor assumes exact reader/writer ARNs and reads only its exact per-project SSM parameter names. Require Role-only modifications with `Replacement=False`; refuse instance, volume, bucket or DNS changes.

The live executor template differs from the repository's UserData. A full-template update would propose conditional instance and volume-attachment replacement. Use `python3 infra/aws/project_iam.py plan --stack layr-executor-layr` against the fetched live template, followed by `inspect` and `execute` with its exact `--change`. The helper refuses anything except Role Modify with Replacement=False. Converge UserData in a separately reviewed maintenance window.

## Person registration, once

1. **agent — prepare the login and one registration invite.** Confirm igor's intended login with the lead. Do not guess it from an AWS account name. On the gateway through SSM, use the administration wrapper to create one scoped, expiring invite directly in SSM. The command is:

   ```sh
   layr-cloud invite "$IGOR_LOGIN" --ssm-parameter /layr-store/invites/igor/token
   ```

   Check the invite's nonsecret metadata and SSM parameter type. Do not print its value in an agent terminal or send it through agent chat.

2. **igor — retrieve the invite in your own Terminal.** The lead supplies the login and this command. The invite is a transport credential, not a recovery key:

   ```sh
   aws --profile ifdotpy --region eu-north-1 ssm get-parameter \
     --name /layr-store/invites/igor/token --with-decryption \
     --query Parameter.Value --output text
   ```

3. **igor — create your person on the site.** Open `https://cloud.layr.ws`. Choose **Join with an invite**. Enter the agreed login and invite. Choose **Create person key**. Keep the 24 recovery words yourself. Confirm that you saved them, then choose **Create account and sign in**. Do not share the words with an agent.

4. **agent — read only the public person identity.** Read the registered recipient and signing public key through the gateway's stored public person record. Have igor compare the key through a trusted channel. Use those exact public values for the project grant and executor pin. Keep the registration recovery slot encrypted.

5. **agent — retire the single-use invite after registration.** Remove the scoped SSM invite parameter. Preserve the persistent session and origin secrets.

## Repeat for each project

Perform the complete sequence for `layr`, then `layr-cloud`, then `layr-studio`.
Do not overwrite the executor's single-project `profile.env` three times as a substitute for enrolling three projects.

### Import and store creation

6. **igor — choose one project and stop GitHub writes for its move.** Keep the existing GitHub repository. Use the corresponding values:

   ```sh
   PROJECT=layr
   GITHUB_URL=git@github.com:ifdotpy/layr.git
   # Other projects: PROJECT=layr-cloud or layr-studio; use its matching GitHub URL.
   GATEWAY_URL="layr+https://cloud.layr.ws/$PROJECT"
   export LAYR_ROOT=/path/to/your/local/layr-store
   ```

   `LAYR_ROOT` contains machine identities and project states. It must not contain the real paper key.

7. **agent — capture the public Git source boundary.** Read GitHub main's commit ID, reachable commit count and tree ID without changing it. Record all three. This is the comparison target; it is not enough to compare only the last file tree.

   ```sh
   git ls-remote "$GITHUB_URL" refs/heads/main
   git -C /path/to/clean/full/git/clone rev-list --count main
   git -C /path/to/clean/full/git/clone rev-parse 'main^{tree}'
   ```

8. **igor — import with the corrected Git bridge.** Do not use `init --from` on a dirty working tree or a shallow clone. Use the final migration CLI from main `ec8ba0f` or newer:

   ```sh
   layr git clone "$GITHUB_URL" "$PROJECT" --branch main
   cd "$LAYR_ROOT/projects/$PROJECT/lines/main"
   layr git remote add github "$GITHUB_URL"
   layr git remote remove origin
   layr rev-parse head
   ```

   Git remote `github` and Cloud remote `origin` are separate namespaces. Ordinary `layr push origin` publishes encrypted Layr objects. `layr git push github main` updates the optional Git mirror.

9. **igor — create an encrypted local seed and retain its paper key.** Use the final migration CLI from main `ec8ba0f` or newer, then run:

    ```sh
    SEED=/path/to/new/encrypted-seed
    layr remote add seed "file://$SEED" --paper-key-tty
    layr push seed
    ```

    The CLI displays the key only on its controlling terminal and asks you to confirm its final word before publication. Choose a new empty `$SEED` directory outside project lines. This contains the public descriptor and encrypted objects, including encrypted key slots. The paper key itself goes only to your private terminal. It must never enter `$SEED`, another file, agent output or shell history. Do not use the current file-output option for a real project.

10. **igor — share only the public seed path and identity.** Tell the lead that import and initialization are complete. Supply `$SEED/store.json` (public project UUID and recipient) and your machine's public identity:

    ```sh
    layr --project "$PROJECT" remote self --json
    ```

    An agent may now copy the seed's encrypted `.age` objects and public `store.json` to the matching project prefix. It must not read a paper-key file or change the seed descriptor. This order avoids creating a gateway alias before the project's recipient exists.

### Gateway and grants

10. **igor — share only the imported project's public metadata.** Supply the project UUID and the machine's public identity. Keep all paper keys and recovery words private. The public identity command is:

    ```sh
    layr --project "$PROJECT" remote self --json
    ```

11. **agent — configure only that gateway alias and issue grants.** After the lead confirms igor's import step, write `gateway/projects/$PROJECT.json` in the artifacts bucket. It contains the matching bucket, region `eu-north-1`, exact prefix and reader/writer/GC role ARNs, project recipient from the seed descriptor, and hashes of transport grants. It contains no plain grant, paper key or recovery words. Publish the initialized seed with:

    ```sh
    python3 infra/aws/publish_project.py --project "$PROJECT" --seed "$SEED"
    ```

    Run `systemctl start layr-gateway-sync.service` on the gateway through SSM and check its project map.

    For the executor's write grant, run on the gateway through SSM:

    ```sh
    layr-cloud token "$PROJECT" --write --until 30d \
      --ssm-parameter "/layr-store/executor/projects/$PROJECT/write-token"
    ```

    After each issuance, persist the updated public grant hashes before the next gateway sync. Read only that public alias configuration from `/etc/layr-gateway/config.json` through SSM into a local public JSON receipt (including its updated hash arrays). On the Mac, run:

    ```sh
    python3 infra/aws/persist_gateway_grants.py --project "$PROJECT" \
      --issued-project /path/to/public-issued-project.json \
      --artifacts-bucket layr-bootstrap-artifacts-ik3kxdaezcnd
    ```

    Its source is `infra/aws/persist_gateway_grants.py`; it compares the alias identity and uses the S3 ETag to refuse concurrent overwrite. Issue separate owner and runner grants with the same token command and exact parameters `/layr-store/owner/projects/$PROJECT/write-token` and `/layr-store/runner/projects/$PROJECT/write-token`, then persist the public hashes again. Each receiving process loads only its grant. Owner and runner write grants also stay in SSM or the receiving process's environment. Do not put them in project configuration, command arguments, shell startup files or logs. Supply igor's connection command through the lead with its approved environment loading recipe.

12. **igor — connect to the initialized Cloud store and push.** The agent first uploads the encrypted seed into the empty matching prefix, checks descriptor equality and publishes the alias. Create a RAM disk for transport credentials in your Terminal (this disk never holds a real paper key):

    ```sh
    test ! -e /Volumes/LayrTransport
    TOKEN_DEVICE=$(hdiutil attach -nomount ram://131072 | tr -d '[:space:]')
    diskutil eraseDisk APFS LayrTransport "$TOKEN_DEVICE"
    TOKEN_DIR=/Volumes/LayrTransport/private
    mkdir -m 700 "$TOKEN_DIR"
    CLOUD_REPO=/path/to/reviewed/layr-cloud
    cloud_layr() {
      python3 "$CLOUD_REPO/infra/aws/with_gateway_grant.py" \
        --parameter "/layr-store/owner/projects/$PROJECT/write-token" \
        --token-dir "$TOKEN_DIR" -- layr "$@"
    }
    ```

    `with_gateway_grant.py` verifies the attached RAM disk, reads the exact SecureString with AWS profile `ifdotpy` in `eu-north-1`, sets `LAYR_GATEWAY_TOKEN_FILE` for one CLI process, and removes that 0600 file on exit. Use this wrapper for subsequent Cloud operations. Then use your already trusted machine slot:

    ```sh
    cloud_layr remote add origin "$GATEWAY_URL"
    cloud_layr push origin
    ```

    Adding this existing store must not create a new paper key or ask for one. Stop if the descriptor differs. Keep the encrypted local seed until the lead accepts the move, then remove it through the normal local cleanup procedure.

13. **agent — compare public store identity.** The gateway descriptor must name the imported project UUID and its recipient. Record these public fields and the pinned descriptor. Do not open or copy `slots/paper.age` with a real paper key.

### Trust people and machines

14. **igor — grant your registered person access.** Use the recipient from your site's person identity and your chosen login. This operation uses your existing trusted machine slot; it does not need the paper key:

    ```sh
    export LAYR_PERSON_LOGIN="$IGOR_LOGIN"
    layr remote grant seed --person "$IGOR_LOGIN" \
      --recipient "$IGOR_PERSON_RECIPIENT" --invite-out /path/to/encrypted-project-invite.age
    layr access person-key "$IGOR_LOGIN" "$IGOR_SIGNING_PUBLIC"
    layr access grant "person:$IGOR_LOGIN" maintainer
    layr protect main --approvers "person:$IGOR_LOGIN" --approvals 1
    layr push seed
    ```

    **agent — publish the new encrypted slot and invite.** After igor confirms this step, run:

    ```sh
    python3 infra/aws/publish_project.py --project "$PROJECT" --seed "$SEED" \
      --person-login "$IGOR_LOGIN" --person-invite /path/to/encrypted-project-invite.age
    ```

    This reads ciphertext and public metadata only. It refuses a different existing object and preserves transport-grant hashes. The first person grant uses the local seed because the current Cloud CLI grant action requires an unlocked person session.

    The project invite is encrypted. Sign in to the site yourself, open **You > Your invites**, review the pending project identity and choose **Accept**. This updates your encrypted person index in the existing signed-in session. The site action is deployed; do not use a person-key file. Never supply recovery words or a project paper key through an agent command.

15. **igor — trust the executor and laptop runner public identities.** Compare the public values with the independently read machine receipts. Then run these for each project:

    ```sh
    cloud_layr remote trust origin "$EXECUTOR_ID" --public-key "$EXECUTOR_PUBLIC"
    cloud_layr remote trust origin "$MAC_RUNNER_ID" --public-key "$MAC_RUNNER_PUBLIC"
    cloud_layr remote trust origin "$LINUX_RUNNER_ID" --public-key "$LINUX_RUNNER_PUBLIC"
    cloud_layr push origin
    ```

    The executor identity is `01a17428-6b5f-7593-b722-ae3fbaba2957`, public key `c12088572a3a43049e33d4b4936d31c2f6f126cbe8fcb86376679629855c1cc7`.
    Obtain current laptop runner public values from `docs/laptop-runner.md`; compare them before trusting them. Only encrypted machine slots go to the store. No real paper key goes to a runner or executor.

16. **agent — enroll that executor project after igor's trust step.** Load its grant from the exact SSM parameter into its own root-owned 0600 tmpfs runtime file. Clone `layr+https://cloud.layr.ws/$PROJECT` using the executor's existing machine identity. Configure empty `queue.run`, reporter-only check flags, the exact person public-key pin and maintainer permission. Grant the nonroot check reporter reader access. Do not import an owner machine identity.

17. **agent — render and install the reviewed runtime after owner enrollment.** First publish a validated executor release that includes the reviewed `checks.py`; use the normal release installation path. Do not edit an active immutable release. On a staging directory, render only projects whose owner trust steps have finished:

    ```sh
    python3 infra/aws/executor_projects.py --output /path/to/new/runtime-stage \
      --projects layr layr-cloud layr-studio
    ```

    This produces one public JSON profile per project and a daemon drop-in plus a single timer. Install only the completed project's profile initially; add later profiles after their owner steps. Do not enable a profile before its clone and person pin are ready. For enrollment, stage that profile separately, use `enroll_executor.py` with the reviewed `checks.py`, and provide only public arguments:

    ```sh
    python3 /run/m1-tools/enroll_executor.py --project "$PROJECT" \
      --person-login "$IGOR_LOGIN" --person-public "$IGOR_SIGNING_PUBLIC"
    ```

    Its `checks.PROJECTS` directory must contain the staged profile while enrolling; the normal location is `/etc/layr-executor/projects.d`. Stop the legacy checks and transfer timers and services before adding profiles; keep the new timer stopped during enrollment. The helper reads the exact SSM parameter, uses the executor's existing machine slot, and removes its tmpfs token on exit. It creates the nonsecret `ready` marker only after clone, configuration and fsck succeed. It does not accept a paper key.

    After all profiles selected for this installation are enrolled, install the rendered systemd files under `/etc/systemd/system`. The `layr.service.d/projects.conf` drop-in removes the legacy single-token dependency and token environment. Stop and disable `layr-checks.timer` and `layr-transfer.timer`, and stop their services before enabling `layr-projects.timer`. Reload systemd, restart `layr.service`, start `layr-projects.service` once and check its journal for each selected project. Then enable the single new timer. Keep the old profile and units as the single-project path until the lead accepts the new runtime. Do not install these runtime changes during the rehearsal or before igor's steps.

    Each profile holds `project` and `writeTokenParameter` only. Each tick loads a separate root-owned 0600 token into `/run/layr-executor-projects` (tmpfs), then fetches, applies requests, reports pending checks, pushes and writes the index. It removes each token after the project tick. One project's malformed profile, token failure or command failure does not skip later projects; the whole tick reports failure after trying the remaining projects.

18. **igor / agent — enroll both laptop runner machines.** Use the reviewed laptop-runner procedure. On the creator, publish the signed job definitions and the `checks.lines`, `checks.changes`, `checks.platforms` and `checks.privileged` selection policy. For the Layr project, run its reviewed `tools/checks/configure.sh` on the creator and push; agree the corresponding jobs for the other two repositories before enabling their runners. A `check.NAME` definition alone does not select a job. Runner-local flags grant execution permission; they do not select jobs. Read-only check workers receive neither SSM credentials nor write tokens. Configure `queue.run` and privileged flags only on the intended runner. Leave the AWS executor's runner flags false.

### Verify and switch daily work

19. **agent — make a fresh Mac clone with a trusted machine slot.** Obtain this machine's public identity first, let igor trust it, and use a transport grant from the environment. Then:

    ```sh
    layr clone "layr+https://cloud.layr.ws/$PROJECT" "$PROJECT"
    cd "$LAYR_ROOT/projects/$PROJECT/lines/main"
    layr rev-parse head
    layr fsck
    ```

    Export this fresh clone into a disposable local bare Git target using the corrected bridge. Compare main's exact commit ID, reachable commit count and tree with step 7. Failure on any comparison stops the move. Retaining a source machine's Git objects does not prove a fresh Cloud clone preserved history.

20. **agent — prove writes and sync on a new owned line.** Create a test line from main, save one small change, push it to CloudFront, fetch it on a second enrolled machine and compare content. Do not write directly to protected main. Remove the disposable test change through the normal reviewed workflow.

21. **igor — verify the real site and submit one test comment.** Sign in with your own person key. Open this project's imported code and history. Keep recovery words and key material out of any agent-assisted browser session. Submit the agreed comment on the test change.

22. **agent — verify one executor application.** Check the signed executor record's request ID and digest. Run request application again and verify the same request is ignored, with exactly one resulting comment. Verify that both laptop runners can report the intended checks, without running privileged checks on AWS.

23. **igor — verify the GitHub mirror boundary and push.** Confirm that GitHub main still names step 7's expected commit. The bridge must reject unrelated or advanced remote history. Do not force-push:

    ```sh
    layr git push github main
    ```

    Have the agent read GitHub's resulting main commit and tree. Keep the GitHub repository; it becomes a mirror after the lead accepts this project's complete checks.

24. **agent — configure Studio lines with ordinary Cloud remotes.** Studio uses its owned Layr line and the real CLI, with a trusted machine slot and scoped transport grant. For an existing local copy:

    ```sh
    layr remote add origin "layr+https://cloud.layr.ws/$PROJECT"
    layr fetch origin
    layr line new agent-task main
    layr save -m 'task change'
    layr push origin
    ```

    Omit `remote add` when the copy already has the matching pinned remote. A new Studio machine needs step 15's trust step before cloning. Agents never need the project's paper key. They submit exact named states to review and use the normal approval and queue workflow for main.

25. **igor — detach the transport RAM disk.** After your Cloud commands finish, run `hdiutil detach "$TOKEN_DEVICE"`. The transport helper removes each token on command exit; detaching removes the temporary filesystem. Keep your real paper key yourself.

26. **agent — retire temporary grants and verify all three profiles.** Preserve real store data and long-lived machine identities. Remove only test people, test projects and temporary grants. Check each project's independent fetch/apply/push/index cycle. A failure in one project must not suppress the other two. Record final hashes, profiles, store stacks and public readbacks in `infra/aws/DEPLOYED.md`.

## Disposable rehearsal

**agent — use a new synthetic Git repository and synthetic person only.** Create a macOS APFS RAM disk and put every disposable paper key, person key, recovery file and local project under it. Use only `layr-store-m8-final`, with a unique alias/login. Exercise the actor steps in the same order, including a fresh CloudFront clone, push/sync, published site sign-in and one executor-applied encrypted comment. Use a local bare Git target for the disposable mirror; do not create or delete GitHub repositories.

**agent — record the full-history gate honestly.** The initial five-commit merge fixture reproduced the c1d7229 loss of four commits. The reviewed correction is merged as `ec8ba0f`. Repeat the same commit-ID, count and tree comparison with this final CLI during the migration rehearsal. The earlier network rehearsal alone proves transport and callers.

**agent — clean up the rehearsal.** Stop its executor and runner enrollments, remove their per-project grant files, then remove scoped SSM parameters, gateway alias, person, invite and disposable S3 versions. Preserve real project stacks and records. Detach the owned RAM disk. Confirm empty disposable store, empty test enrollment, closed forwarding ports and unchanged protected DNS and prototype distribution.

# Releasing the Datadog Lambda Extension

This guide covers how to build and publish the Lambda extension layer to AWS.

## Prerequisites

### Step 1: Create GitHub OIDC Identity Provider in AWS

1. Go to **IAM → Identity providers → Add provider**

2. Configure the provider:
   - **Provider type:** OpenID Connect
   - **Provider URL:** `https://token.actions.githubusercontent.com`
   - **Audience:** `sts.amazonaws.com`

3. Click **Add provider**

Or via AWS CLI:

```bash
aws iam create-open-id-connect-provider \
  --url https://token.actions.githubusercontent.com \
  --client-id-list sts.amazonaws.com
```

### Step 2: Create IAM Role for GitHub Actions

1. Go to **IAM → Roles → Create role**

2. Select **Web identity** as the trusted entity type

3. Configure:
   - **Identity provider:** `token.actions.githubusercontent.com`
   - **Audience:** `sts.amazonaws.com`

4. Click **Next**, then **Create policy** (in a new tab) with this JSON:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "lambda:PublishLayerVersion",
        "lambda:ListLayerVersions",
        "lambda:AddLayerVersionPermission"
      ],
      "Resource": "arn:aws:lambda:*:YOUR_ACCOUNT_ID:layer:Tero-Datadog-Extension*"
    },
    {
      "Effect": "Allow",
      "Action": "sts:GetCallerIdentity",
      "Resource": "*"
    }
  ]
}
```

5. Name the policy `LambdaLayerPublish` and create it

6. Back in the role creation, attach the `LambdaLayerPublish` policy

7. Name the role (e.g., `GitHubActionsLambdaLayerPublish`) and create it

8. **Edit the trust policy** to restrict to your repository. Go to the role →
   Trust relationships → Edit:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Federated": "arn:aws:iam::YOUR_ACCOUNT_ID:oidc-provider/token.actions.githubusercontent.com"
      },
      "Action": "sts:AssumeRoleWithWebIdentity",
      "Condition": {
        "StringEquals": {
          "token.actions.githubusercontent.com:aud": "sts.amazonaws.com"
        },
        "StringLike": {
          "token.actions.githubusercontent.com:sub": "repo:YOUR_ORG/YOUR_REPO:*"
        }
      }
    }
  ]
}
```

Replace:

- `YOUR_ACCOUNT_ID` with your AWS account ID
- `YOUR_ORG/YOUR_REPO` with your GitHub org and repo (e.g.,
  `usetero/datadog-lambda-extension`)

### Step 3: Add Role ARN to GitHub Secrets

1. Copy the role ARN (e.g.,
   `arn:aws:iam::123456789012:role/GitHubActionsLambdaLayerPublish`)

2. Go to your GitHub repository **Settings → Secrets and variables → Actions**

3. Add a new secret:
   - **Name:** `AWS_ROLE_ARN`
   - **Value:** Your role ARN

## Versioning

Releases track DataDog's upstream releases. Upstream publishes `v<N>` tags, so a
Tero release named for upstream v119 contains upstream v119.

An AWS layer version is a single integer, and AWS only ever appends to it — you
cannot ask for version 119. So the upstream version goes in the layer **name**,
and the layer **version** integer is the patch number:

| Release | Layer name                      | Layer version |
| ------- | ------------------------------- | ------------- |
| v119    | `Tero-Datadog-Extension-119`    | 1             |
| v119.2  | `Tero-Datadog-Extension-119`    | 2             |
| v120    | `Tero-Datadog-Extension-120`    | 1             |

This keeps the upstream number exact in every region without publishing filler
versions to advance a counter, and it gives patches somewhere to live. A new
region starts at patch 1 for whatever upstream versions you publish there, so
patch numbers can differ per region; the upstream number never does.

## Staying level with upstream

`.github/workflows/upstream-release-watch.yml` runs daily at 13:00 UTC and can
be triggered by hand. It compares DataDog's latest release with two things: the
version `main` contains (recorded in `.upstream-version`) and whether
`Tero-Datadog-Extension-<N>` is published.

| State                                     | What the watch does                  |
| ----------------------------------------- | ------------------------------------ |
| `main` is behind upstream                 | Opens a PR merging upstream `v<N>`   |
| `main` is level, `v<N>` not published     | Publishes `v<N>` to all regions      |
| Both level                                | Nothing                              |

A release therefore only happens after a human merged the upstream PR and CI was
green. When the merge conflicts the watch opens an issue instead of a PR, because
resolving an upstream merge needs judgement — stale lock entries and API changes
that only clippy catches.

To check parity without publishing, run it by hand with `release` unticked.

`.upstream-version` is the record of what `main` contains. The merge PR bumps it;
set it by hand if you merge upstream without the watch.

## Releasing via GitHub Actions

### Option 1: Tag-based release (recommended)

```bash
git tag v119        # first release of upstream v119
git push origin v119

git tag v119.2      # patch on top of upstream v119
git push origin v119.2
```

A bare `v119` publishes whatever patch comes next. A `v119.2` tag asserts the
release lands on patch 2 and fails **before** publishing if it would not — a
layer version cannot be renumbered after the fact.

Tag-based releases publish both architectures to every region in
`DEFAULT_REGIONS` (see `.github/workflows/release-extension.yml`): all four US
regions and the five EU regions that AWS enables by default.

Opt-in regions (eu-south-1, eu-south-2, eu-central-2) are excluded, because
publishing to a region the account has not enabled fails the job. Add them to
`DEFAULT_REGIONS` once they are enabled.

### Option 2: Manual release

1. Go to **Actions → Release Lambda Extension → Run workflow**

2. Configure the release options:

   | Option          | Description                            | Example                         |
   | --------------- | -------------------------------------- | ------------------------------- |
   | `version`       | Upstream version, patch optional       | `119` or `119.2`                |
   | `regions`       | Comma-separated AWS regions            | `us-east-1,us-west-2,eu-west-1` |
   | `architectures` | Which architectures to build           | `amd64,arm64`                   |
   | `dry_run`       | Build without publishing to AWS        | `false`                         |

3. Click **Run workflow**

4. After completion, find the Layer ARNs in the workflow summary

Manual runs tick `dev` by default, which appends `-dev` to the layer name so they
cannot overwrite a real release. Untick it to publish prod names by hand.

Tag pushes and watch-driven releases always publish prod names.

## Layer Naming Convention

| Architecture | Trigger          | Layer Name                           |
| ------------ | ---------------- | ------------------------------------ |
| amd64        | tag              | `Tero-Datadog-Extension-119`         |
| arm64        | tag              | `Tero-Datadog-Extension-119-ARM`     |
| amd64        | manual           | `Tero-Datadog-Extension-119-dev`     |
| arm64        | manual           | `Tero-Datadog-Extension-119-ARM-dev` |

Substitute the upstream version you are releasing for `119`.

## Using the Published Layer

### Find your Layer ARN

After publishing, the Layer ARN follows this format:

```
arn:aws:lambda:REGION:ACCOUNT_ID:layer:LAYER_NAME:VERSION
```

Example:

```
arn:aws:lambda:us-east-1:123456789012:layer:Tero-Datadog-Extension-119-ARM:1
```

### Update a Lambda function

**AWS CLI:**

```bash
aws lambda update-function-configuration \
  --function-name my-function \
  --layers "arn:aws:lambda:us-east-1:123456789012:layer:Tero-Datadog-Extension-119-ARM:1"
```

**Terraform:**

```hcl
resource "aws_lambda_function" "example" {
  # ... other configuration ...

  layers = [
    "arn:aws:lambda:us-east-1:123456789012:layer:Tero-Datadog-Extension-119-ARM:1"
  ]
}
```

**AWS SAM:**

```yaml
MyFunction:
  Type: AWS::Serverless::Function
  Properties:
    Layers:
      - arn:aws:lambda:us-east-1:123456789012:layer:Tero-Datadog-Extension-119-ARM:1
```

**Serverless Framework:**

```yaml
functions:
  myFunction:
    layers:
      - arn:aws:lambda:us-east-1:123456789012:layer:Tero-Datadog-Extension-119-ARM:1
```

## Local Build and Manual Publish

If you need to build and publish manually without GitHub Actions:

### Build the layer

```bash
# For ARM64
ARCHITECTURE=arm64 FIPS=0 ALPINE=0 DEBUG=0 ./scripts/build_bottlecap_layer.sh

# For AMD64
ARCHITECTURE=amd64 FIPS=0 ALPINE=0 DEBUG=0 ./scripts/build_bottlecap_layer.sh
```

The layer zip will be created in `.layers/`.

### Publish to AWS

```bash
# Set your region
export AWS_REGION=us-east-1

# Publish ARM64 layer
aws lambda publish-layer-version \
  --layer-name "Tero-Datadog-Extension-119-ARM" \
  --description "Tero fork of Datadog Lambda Extension" \
  --zip-file "fileb://.layers/datadog_extension-arm64.zip" \
  --compatible-architectures arm64 \
  --region $AWS_REGION

# Publish AMD64 layer
aws lambda publish-layer-version \
  --layer-name "Tero-Datadog-Extension-119" \
  --description "Tero fork of Datadog Lambda Extension" \
  --zip-file "fileb://.layers/datadog_extension-amd64.zip" \
  --compatible-architectures x86_64 \
  --region $AWS_REGION
```

## Troubleshooting

### Build fails with Boost download error

The Boost download URL may change. Check `images/Dockerfile.bottlecap.compile`
and update the SourceForge URL if needed.

### Build fails with policy-rs compilation error on ARM64

Ensure you're using `policy-rs` version 1.1.1 or later, which includes the ARM64
compatibility fix.

### Release fails with "would publish patch N, but M was requested"

A `v119.2` tag asserts the release lands on patch 2. The check runs before
publishing, because a layer version cannot be renumbered once it exists. The
error reports the patch that would actually be published — re-tag with that
number, or drop the patch (`v119`) to accept whatever comes next.

Patch numbers are per region, so a region added later sits behind the others.
The upstream number in the layer name is unaffected.

### Permission denied when publishing

Verify your IAM role has the required `lambda:PublishLayerVersion` permission
and the trust policy allows your repository.

### OIDC authentication fails

1. Verify the OIDC provider exists in IAM → Identity providers
2. Check the trust policy has the correct `sub` claim for your repo
3. Ensure `AWS_ROLE_ARN` secret is set correctly in GitHub

## Multi-Region Deployment

Releases go to every region in `DEFAULT_REGIONS` by default. To publish to a
different set, pass them comma-separated to a manual run:

```
us-east-1,us-west-2,eu-west-1,ap-southeast-1
```

Each region will get its own copy of the layer. Lambda functions must use a
layer from the same region they're deployed in.

Adding a region needs no catch-up work: the upstream version lives in the layer
name, so a new region publishes `Tero-Datadog-Extension-119:1` directly.

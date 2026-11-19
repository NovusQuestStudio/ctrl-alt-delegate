# Code signing (maintainers)

Release builds can be signed with **Azure Artifact Signing** so Windows shows the publisher's name
instead of "Unknown publisher". This page is the one-time setup. Nothing here is secret: the values
you copy into GitHub identify your Azure resources, and GitHub signs in to Azure with a short-lived
token, so there is no password or key to store.

## What it costs and what it fixes

- **Cost:** the Basic plan is about **$9.99 a month** and includes 5,000 signatures (a release uses
  about 5). Billing starts when you create the signing account, even before your ID is verified.
  Check the current price in the [Azure pricing page](https://azure.microsoft.com/pricing/details/artifact-signing/).
- **Who can use it:** individual developers in the **US or Canada**, with a paid (pay-as-you-go)
  Azure subscription. Free and trial subscriptions are refused.
- **What changes:** Windows shows your legal name as the publisher. SmartScreen may still warn
  about a new download until it has seen enough downloads; signing makes that reputation build up
  under your name.

## Part 1: Azure (you, in the browser, about 30 minutes plus waiting for ID checks)

1. **Subscription.** In the [Azure portal](https://portal.azure.com), use or create a
   pay-as-you-go subscription. Your billing account must be of type **Individual**, and its legal
   name and address must match your government ID exactly.
2. **Turn on the service.** Subscription → **Resource providers** → find `Microsoft.CodeSigning` →
   **Register**.
3. **Create the signing account.** Search for **Artifact Signing Accounts** → **Create**.
   - Resource group: new, for example `ctrl-alt-delegate-signing`.
   - Name: 3–24 letters and numbers, starting with a letter, for example `ctrlaltdelegate`.
   - Region: for example **East US**. Note it; it decides the endpoint below.
   - Pricing tier: **Basic**.
4. **Give yourself permission to verify.** On the new account → **Access control (IAM)** → **Add
   role assignment** → **Artifact Signing Identity Verifier** → yourself.
5. **Verify your identity.** On the account → **Identity validations** → choose **Individual** →
   **New identity** → **Public**. Your name and address come from the billing account. When the
   status says **Action Required**, open the link, verify with your passport, driver's licence or
   state ID, and add the Verified ID to the **Microsoft Authenticator** app when it asks. This can
   take from a few minutes to 30 business days.
6. **Create the certificate profile** (after the status is **Completed**). On the account →
   **Certificate profiles** → **Create** → **Public Trust**. Name it, for example `release`, and pick
   your completed identity validation.

## Part 2: Let GitHub sign in to Azure

1. **App registration.** Search **Microsoft Entra ID** → **App registrations** → **New
   registration**. Name it `ctrl-alt-delegate-github`, keep the defaults, and register. Note the
   **Application (client) ID** and **Directory (tenant) ID**.
2. **Trust GitHub releases.** In the app → **Certificates & secrets** → **Federated credentials** →
   **Add credential** → **GitHub Actions deploying Azure resources**:
   - Organization: `NovusQuestStudio`, Repository: `ctrl-alt-delegate`
   - Entity type: **Environment**, name: `release`
3. **Allow it to sign.** Back on the signing account → **Access control (IAM)** → **Add role
   assignment** → **Artifact Signing Certificate Profile Signer** → select the app
   `ctrl-alt-delegate-github`.
4. **GitHub environment.** In the repository → **Settings** → **Environments** → **New environment**
   → `release`. Optionally require your approval before it runs. Under it, add these
   **environment variables** (not secrets; none of them are sensitive):

   | Name | Value |
   |---|---|
   | `AZURE_CLIENT_ID` | Application (client) ID from step 1 |
   | `AZURE_TENANT_ID` | Directory (tenant) ID from step 1 |
   | `AZURE_SUBSCRIPTION_ID` | Your subscription ID |
   | `SIGNING_ENDPOINT` | `https://eus.codesigning.azure.net/` for East US (`wus2`, `cus`, … for other regions) |
   | `SIGNING_ACCOUNT` | The signing account name, for example `ctrlaltdelegate` |
   | `SIGNING_PROFILE` | The certificate profile name, for example `release` |

## Part 3: The release workflow

With those in place, the release workflow signs `ctrl-alt-delegate.exe` and every copy of `cad.exe`
before it zips them, using [azure/artifact-signing-action](https://github.com/Azure/artifact-signing-action).
If `SIGNING_ACCOUNT` isn't set, it skips signing and builds an unsigned release as before.

Certificates last 72 hours and renew automatically; every signature is timestamped, so signed
releases stay valid after that. To check a signed file, right-click it → **Properties** →
**Digital Signatures**, or run `Get-AuthenticodeSignature .\ctrl-alt-delegate.exe` in PowerShell.

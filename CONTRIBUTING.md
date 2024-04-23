<!-- markdownlint-disable MD033 MD013 -->
# Contributing to Casper - Guideline Document

Welcome! If you are visiting this page, you're likely interested in contributing to the development of Casper. We are thrilled to have you on board.

Casper is a strong advocate of community participation and warmly welcomes contributions in various forms.

There is always a need for improvement in our documentation, creating tutorials, and writing tests. You can contribute to Casper by engaging in development, code reviews, and enhancing documentation.

## Code of Conduct

The Casper [Code of Conduct](https://github.com/casper-ecosystem/.github/blob/main/profile/CODE_OF_CONDUCT.md) governs the conduct expected in this project. By participating, you agree to adhere to this code.

## How Can I Contribute?

### 1. Reporting Bugs

If you encounter issues with `casper-node`, please file an issue in the repository using the specified bug report template. Providing detailed information will help expedite the resolution.

#### Before Submitting A Bug Report

- Confirm the issue is related to `casper-node` by consulting the documentation and FAQs.
- Ensure that the problem has not been previously reported by searching through existing issues.
- If a similar issue exists but remains unresolved, consider contributing to that thread instead of starting a new one.

#### How Do I Submit A (Good) Bug Report?

A well-crafted bug report should clearly outline the problem by following these steps:

- **Use a descriptive title** to help pinpoint the issue.
- **Detail the exact steps to reproduce the problem** so that we can replicate it.
- **Provide specific examples**, such as code snippets, links to files, or GitHub projects that are part of the test case.
- **Describe the observed behavior** and how it differed from your expectations.

### 2. Code Contributions

Whether fixing bugs or developing new features, your contributions are highly valued.

If you are new and looking for simpler tasks, explore the issues labeled as `good first issue`.

You can also check out `beginner` and `help-wanted` issues.

Here’s how you can contribute code:

#### Steps to Submit Code

1. **Fork the repository**: As a contributor, you should `fork` the repository, work on your fork, and submit a pull request. The pull request will undergo review and potentially be merged into the main repository.

   - **1.1** Create a fork from the `dev`, `feature`, or `release-feature` branch for your changes.
   - Note that the naming convention for a Release/Feature branch includes:
     - `release-1.4.6` for Major releases (e.g., Protocol release).
     - `feat-1.5.0` for Feature releases.

2. **Make your changes**: Ensure your code complies with the existing style and architecture. Keep your changes focused, and if they are broad, consider dividing them into smaller PRs for easier review.
3. **Test your changes**: Execute existing tests and add new ones as needed. Your code should pass all existing tests, and new functionalities should come with accompanying tests.
   - **Step 3.1:** Enable Automated Testing [Refer to GitHub’s [Automating builds and tests - GitHub Docs](https://docs.github.com/en/actions/automating-builds-and-tests) for more information].
   - **Step 3.2:** Confirm that tests have been automatically executed.
   - **Step 3.3:** Introduce a test that highlights a problem.
     - **Step 3.3.1:** Report an issue on GitHub.
     - **Step 3.3.2:** Address the broken test and proceed to Step 4.
4. **Document your changes**: Update the documentation to reflect any new behaviors, features, or APIs.
5. **Submit a pull request**: 
   - Provide a concise list of what you have done. Mention relevant issue numbers, and ensure your PR description effectively communicates the problem and solution. Include any pertinent screenshots or output snippets.
   - The Casper Team will review the Pull Request and accept it if it meets the required standards of code quality, test coverage, and security.

   - **Step 5.1:** Git Rules

       We employ `gitchangelog` across all our repositories for maintaining change logs. It is essential to follow the specified convention when composing commit messages. Your pull request may not be approved if you do not adhere to this convention.

   - **Step 5.2:** Commit Message Convention
     
       Consider organizing your commits into rough sections:
     - by purpose (e.g., new, fix, change).
     - by entity (e.g., doc, sdk, code).
     - by audience (e.g., developers, testers, users).
     
     Additionally, you might tag some commits as:
     - `minor`, for minor changes that do not need to be included in your changelog.
     - `refactor`, for significant structural changes that do not introduce new features.
     - `api`, to denote API changes or new APIs.

        **Casper Pull Request Workflow:**

        Refer to the Casper Pull Request Workflow below;

        ![Casper PR Workflow](/casper-node/images/casper_pull_request_flow.png)

### 3. Enhancement Proposals (CEP)

To propose significant changes or enhancements to the Casper Node, submit a Casper Enhancement Proposal (CEP). The CEP process provides a structured framework for new features, fostering organized discussion and review.

#### How to Submit a CEP

1. **Familiarize Yourself**: Review the [CEP README](https://github.com/casper-network/ceps/blob/main/README.md) to understand the process and expectations.
2. **Draft Your Proposal**: Write a draft using the provided [template](https://github.com/casper-network/ceps/blob/main/0000-template.md).
3. **Submit a Pull Request**: File a PR in the [CEP repository](https://github.com/casper-network/ceps) with your proposal. Follow all submission guidelines.
4. **Discussion and Review**: The community and maintainers will evaluate your proposal. Be receptive to feedback and ready to make revisions.
5. **Final Decision**: After thorough discussion, the maintainers will decide on the acceptance of the CEP. If approved, it will be merged into the repository and implemented accordingly.

### 4. Documentation

Enhancing documentation makes the project more accessible and user-friendly. Here's how you can contribute:

- **Identify gaps**: Evaluate existing documentation for completeness and precision. Identify areas lacking content or needing clarification.
- **Make your changes**: Revise existing documents or write new ones to address identified gaps. Ensure clarity and comprehensibility in your documentation.
- **Follow the style guide**: Adhere to the [Casper Writing and Editing Style Guide](https://github.com/casper-network/docs/blob/dev/writing-style-guide) to ensure consistency in voice and formatting across all documents.

### 5. Testing

Interested in testing our documentation, guides, and developer tools? Visit the following pages:

- [Why Build on Casper](https://docs.casper.network/resources/build-on-casper/introduction/).
- [Getting Started with Rust](https://docs.casper.network/developers/writing-onchain-code/getting-started/).
- [Setting up a Local Network with NCTL](https://docs.casper.network/developers/dapps/setup-nctl/).
- [Testing Smart Contracts with NCTL](https://docs.casper.network/developers/dapps/nctl-test/).

## Best Practices for Contributions

- Adhere to the project's style and contribution guidelines.
- Write clear, concise commit messages that effectively communicate the purpose and context of your changes.
- Participate in code reviews if you contribute code.
- Engage with the community to obtain feedback and refine your contributions.

## Additional Resources

For further information on setting up and configuring a development environment for `casper-node`, visit the [official repository](https://github.com/casper-network/casper-node).

We appreciate your contributions to the Casper Node! Your efforts significantly enhance the growth and improvement of our network.

We welcome your insights and feedback from your testing experiences. Please let us know by submitting issues in the respective GitHub repositories.

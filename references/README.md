# Reference source

references/vinegar is a pinned Git submodule containing the Vinegar source code.

Clone this repository with its reference source:

    git clone --recurse-submodules https://github.com/CheckPickerUpper/roblox-studio-linux-launcher.git

If the main repository is already cloned, initialize the reference with:

    git submodule update --init --depth 1

Useful paths:

- references/vinegar/cmd/vinegar
- references/vinegar/internal
- references/vinegar/layer

To review a newer Vinegar revision, update the submodule and commit the resulting pointer:

    git submodule update --remote references/vinegar
    git -C references/vinegar log -1 --oneline
    git add references/vinegar
    git commit -m "Update Vinegar reference"

Vinegar is GPL-3.0 licensed. This checkout is for reading and comparison only.

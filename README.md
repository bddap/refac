[![Crate](https://img.shields.io/crates/v/refac.svg)](https://crates.io/crates/refac)

# refac: Automatically edit text.

The workflow:
- Select some text.
- Run the command, write instructions on what you want changed.
- Never edit text directly again.

This tool calls the openai api. You'll need your own api key to use it.
Use `refac login` to enter your api key. It will be saved in your home directory
for future use.

This tool uses your openai account so usage is not exactly free. It uses the gpt-4 model
chat completion endpoint on the order of ~$0.10 per completion.
You can use https://platform.openai.com/account to see your usage.

## SETUP

```bash
# This tool can be installed using cargo.
cargo install refac

# Enter your api key it will be saved to your drive for future use.
refac login
```

## Try it out

```bash
> refac tor 'The quick brown fox jumps over the lazy dog.' 'convert to all caps'
THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG.

> refac tor '
def add(a: int, b: int):
    return a + b
' 'turn this into a command line program that accepts a and b as arguments, printing the result'`
# I've transformed your `add` function into a command-line script that accepts two integer arguments and prints their sum.
# Based on the syntax of your code, I assume you're using Python. If this is incorrect, please let me know.
# Run the script with `python add.py <a> <b>` where `<a>` and `<b>` are the integers you want to add.
# --refac

import sys

def add(a: int, b: int):
    return a + b

if __name__ == "__main__":
    if len(sys.argv) != 3:
        print("Usage: python add.py <a> <b>")
        sys.exit(1)

    try:
        a = int(sys.argv[1])
        b = int(sys.argv[2])
    except ValueError:
        print("Both arguments must be integers.")
        sys.exit(1)

    result = add(a, b)
    print(f"The result of {a} + {b} is {result}.")

> refac tor '
fn factorial(a: usize) -> usize {
    if a <= 1 {
        1
    } else {
        a * factorial(a - 1)
    }
}
' 'switch to an iterative implementation'
fn factorial(a: usize) -> usize {
    // Switched to an iterative implementation using a loop.
    // --refac
    let mut result = 1;
    for i in 1..=a {
        result *= i;
    }
    result
}

> refac tor '
fn factorial(a: usize) -> usize {
	let mut result = 1;
	for i in 1..=a {
		result *= i;
	}
	result
}
' 'use an iterator and .fold()'
fn factorial(a: usize) -> usize {
    (1..=a).fold(1, |acc, x| acc * x)
}

> refac tor '' 'implement hello world in rust'
// Here's a simple "Hello, World!" program in Rust.
// Compile it with `rustc hello_world.rs` and run it with `./hello_world`.
// --refac

fn main() {
    println!("Hello, World!");
}

> refac tor '
Hey [Name],

I\'m writing to ask if you can lend me $100. I promise I\'ll pay you back as soon as possible.
Thanks,

[Your Name]
' 'make the email more formal and appropriate for a professional setting'
Subject: Request for Temporary Financial Assistance

Dear [Name],

I hope this email finds you well. I am reaching out to request a temporary financial loan of $100. Due to unforeseen circumstances, I find myself in a situation where I require immediate support. Please know that I do not take this request lightly, and I understand the responsibility that comes with borrowing money.

I would like to emphasize my commitment to repaying the loan as promptly as possible. Your assistance during this challenging time would be greatly appreciated, and I assure you that I am doing everything in my power to improve my financial situation.

Should you have any concerns, questions, or require additional information, please do not hesitate to reach out to me. I am more than happy to discuss the matter further.

Thank you very much for your time and consideration. I eagerly await your response.

Sincerely,

```

## Using Refac From Your Favorite Text Editor

First, make sure you have:
- [ ] installed refac
- [ ] entered your [api key](https://platform.openai.com/account/api-keys) using `refac login`

### Emacs

After installing and logging in add this chunk of flim-flam to your init.el:

```elisp
(defun refac-git-style-diff (a b)
  (with-temp-buffer (let ((temp-file-a (make-temp-file "a"))
                          (temp-file-b (make-temp-file "b")))
                      (unwind-protect (progn (write-region a nil temp-file-a)
                                             (write-region b nil temp-file-b)
                                             (call-process "diff" nil t nil "-u" temp-file-a temp-file-b)
                                             (buffer-string))
                        (delete-file temp-file-a)
                        (delete-file temp-file-b)))))

(defun refac-filter-diff-output (diff-output)
  (with-temp-buffer (insert diff-output)
                    (goto-char (point-min))
                    (while (not (eobp))
                      (let ((line
                             (buffer-substring-no-properties
                              (line-beginning-position)
                              (line-end-position))))
                        (if (or (string-prefix-p "--- " line)
                                (string-prefix-p "+++ " line)
                                (string-prefix-p "\\ No newline at end of file" line))
                            (delete-region (line-beginning-position)
                                           (1+ (line-end-position)))
                          (forward-line))))
                    (buffer-string)))


(defun refac-call-executable (selected-text transform)
  (let (result exit-status refac-executable)
    (setq refac-executable (executable-find "refac"))
    (if refac-executable (with-temp-buffer
                           (setq exit-status (call-process refac-executable nil t nil "tor" selected-text transform))
                           (setq result (buffer-string)))
      (error
       "refac executable not found"))
    (if (zerop exit-status) result
      (error
       "refac returned a non-zero exit status: %d. Error: %s"
       exit-status
       result))))

(defun refac (start end)
  (interactive "r")
  (let* ((selected-text
          (buffer-substring-no-properties
           start
           end))
         (transform (read-string "Enter transformation instruction: ")))
    (let ((result (refac-call-executable selected-text transform)))
      (delete-region start end)
      (insert result)
      (let ((diff-output (refac-git-style-diff selected-text result)))
        (message (refac-filter-diff-output diff-output))))))
```

And bind the function to a key if you like that sort of thing.

```elisp
(global-set-key (kbd "C-c r") 'refac)
```

### Not Emacs

Your contrubutions are welcome!

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

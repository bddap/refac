[![Crate](https://img.shields.io/crates/v/refac.svg)](https://crates.io/crates/refac)

# refac: Automatically edit text.

The workflow:
- Select some text.
- Run the command, write instructions on what you want changed.
- Enjoy the sassy comments.

This tool calls the openai api. You'll need your own api key to use it.
Use `refac login` to enter your api key. It will be saved in your home directory
for future use. See [your api usage](https://platform.openai.com/account) .

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

```}
> refac tor '' 'write fizzbuzz in rust'
// Oh, great. The ultimate test of awesomeness. *eyeroll*
// Fine. Here's a sparkling FizzBuzz implementation forged with pure Rustian might do the trick.
// Run with `cargo run`.
// --refac

fn main() {
    for i in 1..=100 {
        match (i % 3, i % 5) {
            (0, 0) => println!("FizzBuzz"),
            (0, _) => println!("Fizz"),
            (_, 0) => println!("Buzz"),
            _ => println!("{i}"),
        }
    }
}



## Using Refac From Your Favorite Text Editor

First, make sure you have:
- [ ] installed refac
- [ ] entered your [api key](https://platform.openai.com/account/api-keys) using `refac login`

### Emacs

After installing and logging in add this chunk of flim-flam to your init.el:

*Note this code assumes `init.el` is using lexical-binding.
Make sure `;; -*- lexical-binding: t -*-` is added to the top of `init.el`.
Otherwise the following code won't work.*

```elisp
(defun refac-call-executable-async (selected-text transform callback)
  "Asynchronously call the refac executable, passing SELECTED-TEXT and TRANSFORM.
CALLBACK is a function taking two parameters: EXIT-STATUS and RESULT."
  (let ((refac-executable (executable-find "refac"))
        (temp-buf (generate-new-buffer "*refac-output*")))
    (if refac-executable
        (make-process
         :name "refac-async"
         :buffer temp-buf
         :command (list refac-executable "tor" selected-text transform)
         :sentinel
         (lambda (proc _event)
           (when (memq (process-status proc) '(exit signal))
             (let ((buf (process-buffer proc)))
               ;; Ensure the buffer is still alive before attempting to read from it
               (when (buffer-live-p buf)
                 (with-current-buffer buf
                   (let ((exit-status (process-exit-status proc))
                         (result (buffer-string)))
                     (kill-buffer buf)
                     (funcall callback exit-status result))))))))
      (error "refac executable not found"))))

(global-set-key (kbd "C-c r") 'refac)

(defun refac (beg end transform)
  "Perform the refac transform in place, inserting merge markers inline, asynchronously.
The overlay will display the transform prompt until the results arrive."
  (interactive "r\nMTransform: ")
  (let* ((buffer (current-buffer))
         (original-text (buffer-substring-no-properties beg end))
         ;; Insert merge markers.
         (_ (goto-char end))
         (_ (insert "\n=======\n\n"))
         (overlay (make-overlay (- (point) 1) (point) buffer t nil))
         (_ (insert ">>>>>>> TRANSFORMED\n"))
         (_ (goto-char beg))
         (_ (insert "<<<<<<< ORIGINAL\n"))
         (_ (smerge-mode 1)))
    ;; Show the transform prompt in the overlay until the async call finishes.
    (overlay-put overlay 'display
                 (concat "Running refac...\nTransform: " transform "\n"))
    (refac-call-executable-async
     original-text
     transform
     (lambda (exit-status result)
       (with-current-buffer buffer
         (let ((saved-point (point)))
           (goto-char (overlay-start overlay))
           ;; Once results arrive, we remove the overlay and insert the result.
           (delete-overlay overlay)
           (insert result)
           (goto-char saved-point)
           (unless (zerop exit-status)
             (error "refac returned a non-zero exit status: %d. Error: %s"
                    exit-status result))))))))
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

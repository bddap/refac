[![Crate](https://img.shields.io/crates/v/refac.svg)](https://crates.io/crates/refac)

# refac: Automatically edit text.

The workflow:
- Select some text.
- Run the command, write instructions on what you want changed.
- Enjoy the sassy comments.

Calls Claude by default — bring your own key and run `refac login` (or set
`ANTHROPIC_API_KEY`). For OpenAI, set `REFAC_PROVIDER=openai` and `OPENAI_API_KEY`.
Optional `provider` / `model` config lives in `~/.config/refac/config.toml`.

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
' 'turn this into a command line program that accepts a and b as arguments, printing the result'
# Another riveting addition machine for the ages. I'll spruce it up with type hints and argparse,
# because apparently I have standards even when you don't.
# Run it with `python add.py <a> <b>`.
# --refac

import argparse


def add(a: int, b: int) -> int:
    return a + b


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Add two integers.")
    parser.add_argument("a", type=int, help="The first integer.")
    parser.add_argument("b", type=int, help="The second integer.")
    args = parser.parse_args()
    print(add(args.a, args.b))

> refac tor '
fn factorial(a: usize) -> usize {
    if a <= 1 {
        1
    } else {
        a * factorial(a - 1)
    }
}
' 'switch to an iterative implementation'
// Recursion is elegant and all, but why summon the stack-overflow goblins when a loop will do?
// Here's your iterative factorial, served fresh.
// --refac
fn factorial(a: usize) -> usize {
    let mut result = 1;
    for i in 2..=a {
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
// Look at you, embracing the functional style! I'm getting all warm and fuzzy inside.
// --refac
fn factorial(a: usize) -> usize {
    (1..=a).fold(1, |result, i| result * i)
}

> refac tor '' 'implement hello world in rust'
// Behold! A masterpiece of unparalleled complexity. Try not to strain yourself.
// --refac
fn main() {
    println!("Hello, world!");
}

> refac tor '' 'write fizzbuzz in rust'
// FizzBuzz, the eternal rite of passage. You've ascended to junior dev status, congrats.
// --refac
fn main() {
    for i in 1..=100 {
        match (i % 3, i % 5) {
            (0, 0) => println!("FizzBuzz"),
            (0, _) => println!("Fizz"),
            (_, 0) => println!("Buzz"),
            _ => println!("{}", i),
        }
    }
}

> refac tor '
Hey [Name],

I'm writing to ask if you can lend me $100. I promise I'll pay you back as soon as possible.
Thanks,

[Your Name]
' 'make the email more formal and appropriate for a professional setting'
Dear [Name],

I hope this message finds you well. I am writing to respectfully request a short-term loan of $100. I would be most grateful for your assistance, and I assure you that I will repay the amount at the earliest possible opportunity.

Please let me know if this is something you would be willing to consider. I am happy to discuss any terms or arrangements that would be convenient for you.

Thank you very much for your time and consideration.

Best regards,

[Your Name]
```

## Using Refac From Your Favorite Text Editor

First, make sure you have:
- [ ] installed refac
- [ ] entered your [API key](https://console.anthropic.com/settings/keys) using `refac login`

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

export function hello(): string {
    return "hello";
}

export const twice = hello() + hello();

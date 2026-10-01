export const faker = new Proxy({}, {get(){return new Proxy(()=>'', {get(){return ()=>''}})}});

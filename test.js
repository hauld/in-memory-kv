const Database = require('./index');

const db = new Database();
const test = {
    context: {
        event: 'ef84cd8b-ac85-49b3-bdb0-0783d058703c',
        type: 'task_error',
        when: 1663800404990,
        content: {
          runId: '328289e1-bcb4-4fd0-9341-d6abf7ad290e',
          task: {
            runId: '03ae15a9-c1dc-4ed5-8189-2048ac3fdb89',
            id: 'task-1',
            type: 'class/taskEntry',
            taskInfo: {
              id: 'task-1',
              type: 'class/taskEntry',
              cache: false,
              content: { init: true },
              coordinates: [ 10, 20 ]
            },
            nip: 0,
            flowId: 'd20d9c86-c0fa-4a6e-90ed-247989cf6331'
          },
          input: { '0': {} }
        }
      }
    }
var key = 1;
function insertLoop(){
    db.set(key.toString(), JSON.stringify(test), 0).then((value) => {
        //console.log(value);
    });
    key++;
    setTimeout(insertLoop, 500);
}

insertLoop();

function loop(){
    db.iter().then((value) => {
        if(value.length > 0)
            console.log(JSON.parse(value));
    })
    setImmediate(loop);
}
loop();
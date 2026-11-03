function first() {}
function second() {
  first();
}
class Old {
  run() {
    second();
  }
}
second();
module.exports = { Old };

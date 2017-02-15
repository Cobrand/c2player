#include <unistd.h>
#include "aml_player.h"

int main() {
	void* p = aml_video_player_create();
	sleep(6);
	aml_video_player_destroy(p);
}
